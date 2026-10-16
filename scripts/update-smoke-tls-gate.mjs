import { mkdtemp, rm } from "node:fs/promises";
import { readFile } from "node:fs/promises";
import { createReadStream } from "node:fs";
import { createServer as createHttpServer } from "node:http";
import { createServer as createHttpsServer } from "node:https";
import { connect } from "node:net";
import { tmpdir } from "node:os";
import { join, posix } from "node:path";
import { URL } from "node:url";

import { FIXED_STABLE_ENDPOINT } from "./public-draft-assets.mjs";
import { releaseAssetNames } from "./release-assets.mjs";
import { createTlsIdentity } from "./update-smoke-tools.mjs";
import { setTimeout, clearTimeout } from "node:timers";

const HOSTNAME = "updates.cyunlab.com";
const LATEST_PATH = "/narrant/channels/stable/latest.json";

/** 从已验证 manifest 生成当前 target 唯一允许的 latest/tag 路由。 */
export function createCandidateGateRoutes(options) {
  const { target, version, directory, manifest } = options;
  if (!["darwin-aarch64", "windows-x86_64"].includes(target)) throw new Error("TLS gate target 无效");
  const names = releaseAssetNames(version);
  const packageName = target === "darwin-aarch64" ? names.updater.darwin : names.updater.windows;
  const expectedUrl = manifest?.platforms?.[target]?.url;
  const expectedPrefix = `https://${HOSTNAME}/narrant/releases/${version}/${target}/`;
  if (typeof expectedUrl !== "string" || !expectedUrl.startsWith(expectedPrefix) || !expectedUrl.endsWith(`-${encodeURIComponent(packageName)}`)) {
    throw new Error("TLS gate manifest package URL 不匹配");
  }
  const packageUrl = new URL(expectedUrl);
  return Object.freeze([
    Object.freeze({ pathname: LATEST_PATH, file: join(directory, names.manifest), contentType: "application/json; charset=utf-8" }),
    Object.freeze({ pathname: packageUrl.pathname, file: join(directory, packageName), contentType: "application/octet-stream" }),
  ]);
}

/** 校验 gate 只能拦截正式 Stable endpoint 与当前 tag 包路径。 */
export function validateGateConfiguration(config) {
  const endpoint = new URL(config.endpoint);
  if (endpoint.href !== FIXED_STABLE_ENDPOINT || endpoint.hostname !== HOSTNAME || endpoint.pathname !== LATEST_PATH) {
    throw new Error("TLS gate 必须使用 shipped Stable endpoint");
  }
  if (!Array.isArray(config.routes) || config.routes.length !== 2) throw new Error("TLS gate 必须精确服务 manifest 与一个 target package");
  const paths = config.routes.map((route) => route.pathname);
  if (new Set(paths).size !== 2 || !paths.includes(LATEST_PATH)) throw new Error("TLS gate route 集合无效");
  for (const route of config.routes) {
    const allowedPath = route.pathname === LATEST_PATH ||
      route.pathname.startsWith(`/narrant/releases/${config.tag.replace(/^v/, "")}/`);
    if (
      typeof route.file !== "string" || !route.file ||
      typeof route.pathname !== "string" ||
      !allowedPath ||
      posix.normalize(route.pathname) !== route.pathname ||
      route.pathname.includes("\\") || route.pathname.includes("?") || route.pathname.includes("#")
    ) {
      throw new Error("TLS gate route 不在固定 OSS Stable/release allowlist");
    }
  }
  return endpoint;
}

/** 合并主错误与清理错误，主错误始终保留为 AggregateError.message。 */
export function combineGateErrors(primary, cleanupErrors) {
  if (!primary && cleanupErrors.length === 0) return undefined;
  if (primary && cleanupErrors.length === 0) return primary;
  if (!primary && cleanupErrors.length === 1) return cleanupErrors[0];
  return new AggregateError([primary, ...cleanupErrors].filter(Boolean), primary?.message ?? "TLS gate 清理失败");
}

/** 在随机环回端口启动服务，不占用系统端口或改变主机路由。 */
async function listen(server) {
  await new Promise((resolve, reject) => {
    server.once("error", reject);
    server.listen({ host: "127.0.0.1", port: 0 }, resolve);
  });
  return server.address().port;
}

/**
 * 只通过测试子进程的 HTTPS_PROXY/SSL_CERT_FILE 接入固定 HTTPS 域名。
 * CA 保持为临时文件，不改变 hosts、证书信任、系统代理或其他应用的网络。
 */
export async function withUpdateSmokeTlsGate(config, action, dependencies = {}) {
  const endpoint = validateGateConfiguration(config);
  if (typeof action !== "function") throw new Error("TLS gate action 缺失");
  const directory = await mkdtemp(join(tmpdir(), "narrant-update-smoke-tls-"));
  const servers = [];
  const sockets = new Set();
  const served = new Set();
  const completed = Promise.withResolvers();
  // 请求失败可以早于等待者注册，先接住拒绝，随后由真实等待传播。
  completed.promise.catch(() => {});
  let primary;
  let result;
  let waitTimer;
  let finalFailure;
  try {
    const identity = await (dependencies.createTlsIdentity ?? createTlsIdentity)({ hostname: endpoint.hostname, directory });
    const routes = new Map(config.routes.map((route) => [route.pathname, route]));
    const https = createHttpsServer({ cert: await readFile(identity.certificate), key: await readFile(identity.key) }, (request, response) => {
      const route = routes.get(request.url);
      request.resume();
      if (request.method !== "GET" || ![endpoint.hostname, `${endpoint.hostname}:443`].includes(request.headers.host) || !route) {
        response.writeHead(404).end();
        return;
      }
      response.writeHead(200, { "content-type": route.contentType, "cache-control": "no-store", connection: "close" });
      const input = createReadStream(route.file);
      input.once("error", (error) => { completed.reject(error); response.destroy(error); });
      response.once("finish", () => {
        served.add(route.pathname);
        if (served.size === routes.size) completed.resolve();
      });
      input.pipe(response);
    });
    servers.push(https);
    https.on("connection", (socket) => { sockets.add(socket); socket.once("close", () => sockets.delete(socket)); });
    const tlsPort = await listen(https);
    const proxy = createHttpServer((_request, response) => response.writeHead(403).end());
    servers.push(proxy);
    proxy.on("connection", (socket) => { sockets.add(socket); socket.once("close", () => sockets.delete(socket)); });
    proxy.on("connect", (request, client, head) => {
      if (request.url !== `${endpoint.hostname}:443`) {
        client.end("HTTP/1.1 403 Forbidden\r\nConnection: close\r\n\r\n");
        return;
      }
      const upstream = connect({ host: "127.0.0.1", port: tlsPort });
      sockets.add(upstream);
      upstream.once("close", () => { sockets.delete(upstream); client.destroy(); });
      client.once("close", () => upstream.destroy());
      upstream.once("error", () => client.destroy());
      client.once("error", () => upstream.destroy());
      upstream.once("connect", () => {
        client.write("HTTP/1.1 200 Connection Established\r\n\r\n");
        if (head.length) upstream.write(head);
        client.pipe(upstream).pipe(client);
      });
    });
    const proxyPort = await listen(proxy);
    result = await action(Object.freeze({
      environment: Object.freeze({
        HTTPS_PROXY: `http://127.0.0.1:${proxyPort}`,
        SSL_CERT_FILE: identity.authority,
        SSL_CERT_DIR: directory,
        NO_PROXY: "",
      }),
      /** 等待真实旧应用完整读取 manifest 与该平台的候选包。 */
      waitForAllRequests: () => {
        waitTimer ??= setTimeout(() => completed.reject(new Error("TLS gate 等待升级请求超时")), config.timeoutMilliseconds ?? 8 * 60 * 1000);
        return completed.promise;
      },
    }));
  } catch (error) {
    primary = error;
  } finally {
    const cleanup = [];
    clearTimeout(waitTimer);
    for (const socket of sockets) socket.destroy();
    for (const server of servers.reverse()) {
      try { await new Promise((resolve, reject) => server.close((error) => error && error.code !== "ERR_SERVER_NOT_RUNNING" ? reject(error) : resolve())); }
      catch (error) { cleanup.push(error); }
    }
    try { await rm(directory, { recursive: true, force: true }); } catch (error) { cleanup.push(error); }
    finalFailure = combineGateErrors(primary, cleanup);
  }
  if (finalFailure) throw finalFailure;
  return result;
}
