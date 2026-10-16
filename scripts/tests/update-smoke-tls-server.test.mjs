import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { once } from "node:events";
import { mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { request } from "node:https";
import { tmpdir } from "node:os";
import path from "node:path";
import process from "node:process";
import test from "node:test";

import { createCandidateGateRoutes } from "../update-smoke-tls-gate.mjs";
import { createUpdateSmokeTlsSystemAdapter } from "../update-smoke-tls-system-adapter.mjs";

/** 连接随机环回端口，同时校验正式域名的 TLS 证书，不修改系统信任或 DNS。 */
async function readGate(port, pathname, ca) {
  return new Promise((resolve, reject) => {
    const pending = request({
      hostname: "127.0.0.1", port, servername: "updates.cyunlab.com", ca,
      path: pathname, headers: { host: "updates.cyunlab.com" },
    }, (response) => {
      let body = "";
      response.setEncoding("utf8");
      response.on("data", (chunk) => { body += chunk; });
      response.once("end", () => resolve({ status: response.statusCode, body }));
      response.once("error", reject);
    });
    pending.once("error", reject);
    pending.end();
  });
}

test("实际 TLS helper 接受当前 Stable 域名并逐字节服务两个允许的路径", { timeout: 20_000 }, async (context) => {
  const directory = await mkdtemp(path.join(tmpdir(), "narrant-tls-server-test-"));
  let child;
  let exited;
  context.after(async () => {
    try {
      if (child?.exitCode === null && child.signalCode === null) child.kill("SIGKILL");
      if (exited) await exited;
    } finally {
      await rm(directory, { recursive: true, force: true });
    }
  });
  const hostname = "updates.cyunlab.com";
  const identity = await createUpdateSmokeTlsSystemAdapter().createTlsIdentity({ hostname, directory });
  const packageName = "narrant-darwin-aarch64-updater.app.tar.gz";
  const packagePath = `/narrant/releases/2.0.0/darwin-aarch64/${"a".repeat(64)}-${packageName}`;
  const manifest = { version: "2.0.0", platforms: { "darwin-aarch64": { url: `https://${hostname}${packagePath}`, signature: "fixture" } } };
  const manifestBytes = JSON.stringify(manifest);
  await writeFile(path.join(directory, "latest.json"), manifestBytes);
  await writeFile(path.join(directory, packageName), "exact-package-bytes");
  const routes = createCandidateGateRoutes({ target: "darwin-aarch64", version: "2.0.0", directory, manifest });
  const routesFile = path.join(directory, "routes.json");
  await writeFile(routesFile, JSON.stringify(routes));
  child = spawn(process.execPath, [
    path.resolve("scripts/update-smoke-tls-gate-server.mjs"),
    "--hostname", hostname, "--certificate", identity.certificate, "--key", identity.key,
    "--routes", routesFile, "--port", "0",
  ], { stdio: ["pipe", "pipe", "pipe"] });
  exited = once(child, "close");
  let stderr = "";
  child.stderr.setEncoding("utf8").on("data", (chunk) => { stderr += chunk; });
  const port = await new Promise((resolve, reject) => {
    let output = "";
    child.once("error", reject);
    child.once("close", () => reject(new Error(`TLS helper 提前退出：${stderr}`)));
    child.stdout.setEncoding("utf8").on("data", (chunk) => {
      output += chunk;
      for (;;) {
        const newline = output.indexOf("\n");
        if (newline < 0) break;
        const event = JSON.parse(output.slice(0, newline));
        output = output.slice(newline + 1);
        if (event.event === "ready") resolve(event.port);
      }
    });
  });
  assert.ok(Number.isInteger(port) && port > 0 && port !== 443);
  const authority = await readFile(identity.authority);
  assert.deepEqual(await readGate(port, routes[0].pathname, authority), { status: 200, body: manifestBytes });
  assert.equal((await readGate(port, "/cyunlab/narrant/releases/latest/download/latest.json", authority)).status, 404);
  assert.deepEqual(await readGate(port, packagePath, authority), { status: 200, body: "exact-package-bytes" });
  child.stdin.end();
  assert.deepEqual(await exited, [0, null]);
});
