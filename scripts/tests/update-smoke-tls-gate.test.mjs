import assert from "node:assert/strict";
import { mkdtemp, readFile, rm, stat, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

import { request as httpRequest } from "node:http";
import { request as httpsRequest, Agent } from "node:https";
import { connect as tlsConnect } from "node:tls";
import { connect } from "node:net";
import {
  combineGateErrors,
  createCandidateGateRoutes,
  validateGateConfiguration,
  withUpdateSmokeTlsGate,
} from "../update-smoke-tls-gate.mjs";

const endpoint = "https://updates.cyunlab.com/narrant/channels/stable/latest.json";

/** 创建两路由所需的最小本地文件与 manifest。 */
async function fixture(context) {
  const directory = await mkdtemp(join(tmpdir(), "narrant-gate-test-"));
  context.after(() => rm(directory, { recursive: true, force: true }));
  const manifest = {
    version: "2.0.0",
    platforms: {
      "darwin-aarch64": {
        url: "https://updates.cyunlab.com/narrant/releases/2.0.0/darwin-aarch64/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa-narrant-darwin-aarch64-updater.app.tar.gz",
        signature: "fixture",
      },
      "windows-x86_64": {
        url: "https://updates.cyunlab.com/narrant/releases/2.0.0/windows-x86_64/bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb-narrant-windows-x86_64-updater.exe",
        signature: "fixture",
      },
    },
  };
  await Promise.all([
    writeFile(join(directory, "latest.json"), JSON.stringify(manifest)),
    writeFile(join(directory, "narrant-darwin-aarch64-updater.app.tar.gz"), "package"),
  ]);
  return { directory, manifest };
}

test("gate routes 只含 shipped latest 与当前 tag/target package", async (context) => {
  const { directory, manifest } = await fixture(context);
  const routes = createCandidateGateRoutes({ target: "darwin-aarch64", tag: "v2.0.0", version: "2.0.0", directory, manifest });
  assert.deepEqual(routes.map((route) => route.pathname), [
    "/narrant/channels/stable/latest.json",
    "/narrant/releases/2.0.0/darwin-aarch64/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa-narrant-darwin-aarch64-updater.app.tar.gz",
  ]);
  assert.doesNotThrow(() => validateGateConfiguration({ endpoint, tag: "v2.0.0", routes }));
  assert.throws(() => validateGateConfiguration({ endpoint: "https://example.com/latest.json", tag: "v2.0.0", routes }), /shipped/);
  assert.throws(() => validateGateConfiguration({ endpoint, tag: "v2.0.0", routes: [routes[0], { ...routes[1], pathname: "/other/repo" }] }), /allowlist/);
});

/** 经真实 CONNECT 隧道读取 HTTPS，不为测试客户端关闭证书或主机名校验。 */
async function readThroughProxy(environment, pathname, trust = true) {
  const proxy = new URL(environment.HTTPS_PROXY);
  const ca = trust ? await readFile(environment.SSL_CERT_FILE) : undefined;
  const agent = new Agent();
  agent.createConnection = (_options, callback) => {
    const tunnel = httpRequest({ host: proxy.hostname, port: proxy.port, method: "CONNECT", path: "updates.cyunlab.com:443" });
    tunnel.once("error", callback);
    tunnel.once("connect", (response, socket) => {
      if (response.statusCode !== 200) { socket.destroy(); callback(new Error("CONNECT refused")); return; }
      const secure = tlsConnect({ socket, servername: "updates.cyunlab.com", ca });
      secure.once("error", callback);
      secure.once("secureConnect", () => callback(null, secure));
    });
    tunnel.end();
  };
  try {
    return await new Promise((resolve, reject) => {
      const request = httpsRequest(`https://updates.cyunlab.com${pathname}`, { agent }, (response) => {
        let body = "";
        response.setEncoding("utf8").on("data", (chunk) => { body += chunk; });
        response.once("end", () => resolve({ status: response.statusCode, body }));
        response.once("error", reject);
      });
      request.once("error", reject);
      request.end();
    });
  } finally { agent.destroy(); }
}

test("真实进程代理验证 TLS、精确包字节并拒绝其他域名和旧路径", { timeout: 20_000 }, async (context) => {
  const { directory, manifest } = await fixture(context);
  const routes = createCandidateGateRoutes({ target: "darwin-aarch64", version: "2.0.0", directory, manifest });
  let environment;
  await withUpdateSmokeTlsGate({ endpoint, tag: "v2.0.0", routes }, async (gate) => {
    environment = gate.environment;
    await assert.rejects(readThroughProxy(environment, routes[0].pathname, false), /certificate|issuer|self.signed/i);
    assert.deepEqual(await readThroughProxy(environment, routes[0].pathname), { status: 200, body: JSON.stringify(manifest) });
    assert.deepEqual(await readThroughProxy(environment, routes[1].pathname), { status: 200, body: "package" });
    assert.equal((await readThroughProxy(environment, "/cyunlab/narrant/releases/latest/download/latest.json")).status, 404);
    const proxy = new URL(environment.HTTPS_PROXY);
    const status = await new Promise((resolve, reject) => {
      const request = httpRequest({ host: proxy.hostname, port: proxy.port, method: "CONNECT", path: "github.com:443" });
      request.once("error", reject);
      request.once("connect", (response, socket) => { socket.destroy(); resolve(response.statusCode); });
      request.end();
    });
    assert.equal(status, 403);
    await gate.waitForAllRequests();
  });
  await assert.rejects(stat(environment.SSL_CERT_FILE), { code: "ENOENT" });
});

test("客户端失败后关闭真实监听端口并删除临时 CA，保留主错误", { timeout: 20_000 }, async (context) => {
  const { directory, manifest } = await fixture(context);
  const routes = createCandidateGateRoutes({ target: "darwin-aarch64", version: "2.0.0", directory, manifest });
  let environment;
  await assert.rejects(withUpdateSmokeTlsGate({ endpoint, tag: "v2.0.0", routes }, async (gate) => {
    environment = gate.environment;
    await readThroughProxy(environment, routes[0].pathname);
    throw new Error("native application failed");
  }), /native application failed/);
  const proxy = new URL(environment.HTTPS_PROXY);
  await assert.rejects(new Promise((resolve, reject) => {
    const socket = connect({ host: proxy.hostname, port: proxy.port });
    socket.once("error", reject);
    socket.once("connect", () => { socket.destroy(); resolve(); });
  }), { code: "ECONNREFUSED" });
  await assert.rejects(stat(environment.SSL_CERT_FILE), { code: "ENOENT" });
});

test("清理错误保留主错误和各项失败", () => {
  const failure = combineGateErrors(new Error("primary"), [new Error("server"), new Error("directory")]);
  assert.ok(failure instanceof AggregateError);
  assert.equal(failure.message, "primary");
  assert.equal(failure.errors.length, 3);
});
