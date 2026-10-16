import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join, posix } from "node:path";
import { URL } from "node:url";

import { FIXED_STABLE_ENDPOINT } from "./public-draft-assets.mjs";
import { releaseAssetNames } from "./release-assets.mjs";

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

/**
 * 在 runner-only 系统边界内安装临时 CA、修改 hosts 并服务两个 byte-exact 文件；
 * 任意失败都按 hosts→server→CA→临时目录顺序恢复。
 */
export async function withUpdateSmokeTlsGate(config, action, systemAdapter) {
  const endpoint = validateGateConfiguration(config);
  if (typeof action !== "function") throw new Error("TLS gate action 缺失");
  for (const method of ["createTlsIdentity", "installCertificateAuthority", "startHttpsGate", "prepareHostnameRoute", "routeHostname", "restoreHostname", "removeCertificateAuthority"]) {
    if (typeof systemAdapter?.[method] !== "function") throw new Error(`TLS system adapter 缺少 ${method}`);
  }
  const directory = await mkdtemp(join(tmpdir(), "narrant-update-smoke-tls-"));
  let identity;
  let server;
  let trusted = false;
  let routed = false;
  let primary;
  let result;
  let finalFailure;
  try {
    identity = await systemAdapter.createTlsIdentity({ hostname: endpoint.hostname, directory });
    await systemAdapter.installCertificateAuthority(identity);
    trusted = true;
    server = await systemAdapter.startHttpsGate({ hostname: endpoint.hostname, routes: config.routes, identity });
    if (typeof server?.waitForAllRequests !== "function" || typeof server?.close !== "function") {
      throw new Error("TLS server adapter contract 无效");
    }
    await systemAdapter.prepareHostnameRoute(endpoint.hostname, "127.0.0.1");
    routed = true;
    await systemAdapter.routeHostname();
    result = await action(Object.freeze({
      /** 等待真实旧应用读取 exact latest.json 与当前 target package。 */
      waitForAllRequests: () => server.waitForAllRequests(),
    }));
  } catch (error) {
    primary = error;
  } finally {
    const cleanup = [];
    if (routed) try { await systemAdapter.restoreHostname(); } catch (error) { cleanup.push(error); }
    if (server) try { await server.close(); } catch (error) { cleanup.push(error); }
    if (trusted) try { await systemAdapter.removeCertificateAuthority(identity); } catch (error) { cleanup.push(error); }
    try { await rm(directory, { recursive: true, force: true }); } catch (error) { cleanup.push(error); }
    finalFailure = combineGateErrors(primary, cleanup);
  }
  if (finalFailure) throw finalFailure;
  return result;
}
