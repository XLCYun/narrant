import assert from "node:assert/strict";
import { Buffer } from "node:buffer";
import { mkdtemp, readFile, rm, stat, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import process from "node:process";
import test from "node:test";

import { buildTemporaryHostsBytes, createUpdateSmokeTlsSystemAdapter, tlsTrustCommandPlan } from "../update-smoke-tls-system-adapter.mjs";
import {
  combineGateErrors,
  createCandidateGateRoutes,
  validateGateConfiguration,
  withUpdateSmokeTlsGate,
} from "../update-smoke-tls-gate.mjs";

const endpoint = "https://github.com/cyunlab/narrant/releases/latest/download/latest.json";

/** 创建两路由所需的最小本地文件与 manifest。 */
async function fixture(context) {
  const directory = await mkdtemp(join(tmpdir(), "narrant-gate-test-"));
  context.after(() => rm(directory, { recursive: true, force: true }));
  const manifest = {
    version: "2.0.0",
    platforms: {
      "darwin-aarch64": {
        url: "https://github.com/cyunlab/narrant/releases/download/v2.0.0/narrant-darwin-aarch64-updater.app.tar.gz",
        signature: "fixture",
      },
      "windows-x86_64": {
        url: "https://github.com/cyunlab/narrant/releases/download/v2.0.0/narrant-windows-x86_64-updater.exe",
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
    "/cyunlab/narrant/releases/latest/download/latest.json",
    "/cyunlab/narrant/releases/download/v2.0.0/narrant-darwin-aarch64-updater.app.tar.gz",
  ]);
  assert.doesNotThrow(() => validateGateConfiguration({ endpoint, tag: "v2.0.0", routes }));
  assert.throws(() => validateGateConfiguration({ endpoint: "https://example.com/latest.json", tag: "v2.0.0", routes }), /shipped/);
  assert.throws(() => validateGateConfiguration({ endpoint, tag: "v2.0.0", routes: [routes[0], { ...routes[1], pathname: "/other/repo" }] }), /allowlist/);
});

test("gate 任意失败都按 hosts/server/CA 顺序恢复", async (context) => {
  const { directory, manifest } = await fixture(context);
  const routes = createCandidateGateRoutes({ target: "darwin-aarch64", tag: "v2.0.0", version: "2.0.0", directory, manifest });
  const events = [];
  const adapter = {
    async createTlsIdentity() { events.push("identity"); return {}; },
    async installCertificateAuthority() { events.push("trust"); },
    async startHttpsGate() {
      events.push("server");
      return { async waitForAllRequests() { events.push("requests"); }, async close() { events.push("close"); } };
    },
    async prepareHostnameRoute() { events.push("prepare-route"); },
    async routeHostname() { events.push("route"); },
    async restoreHostname() { events.push("restore"); },
    async removeCertificateAuthority() { events.push("untrust"); },
  };
  await assert.rejects(() => withUpdateSmokeTlsGate({ endpoint, tag: "v2.0.0", routes }, async (gate) => {
    await gate.waitForAllRequests();
    throw new Error("primary native failure");
  }, adapter), /primary native failure/);
  assert.deepEqual(events, ["identity", "trust", "server", "prepare-route", "route", "requests", "restore", "close", "untrust"]);
});

test("hosts mutation 写后抛错仍执行 armed restore 并保留主错误", async (context) => {
  const { directory, manifest } = await fixture(context);
  const routes = createCandidateGateRoutes({ target: "darwin-aarch64", tag: "v2.0.0", version: "2.0.0", directory, manifest });
  const events = [];
  const adapter = {
    async createTlsIdentity() { events.push("identity"); return {}; },
    async installCertificateAuthority() { events.push("trust"); },
    async startHttpsGate() {
      events.push("server");
      return { async waitForAllRequests() {}, async close() { events.push("close"); } };
    },
    async prepareHostnameRoute() { events.push("prepare-route"); },
    async routeHostname() { events.push("hosts-mutated"); throw new Error("route post-write failure"); },
    async restoreHostname() { events.push("restore"); },
    async removeCertificateAuthority() { events.push("untrust"); },
  };
  await assert.rejects(
    () => withUpdateSmokeTlsGate({ endpoint, tag: "v2.0.0", routes }, async () => {}, adapter),
    /route post-write failure/,
  );
  assert.deepEqual(events, ["identity", "trust", "server", "prepare-route", "hosts-mutated", "restore", "close", "untrust"]);
});

test("cleanup 失败不会覆盖主错误，多个失败收敛为 AggregateError", () => {
  const primary = new Error("primary");
  const failure = combineGateErrors(primary, [new Error("hosts"), new Error("trust")]);
  assert.equal(failure instanceof AggregateError, true);
  assert.equal(failure.message, "primary");
  assert.equal(failure.errors.length, 3);
});

test("hosts bytes 只追加 exact github.com，拒绝已有映射和其他 host", () => {
  const original = Buffer.from("127.0.0.1 localhost\n");
  assert.equal(buildTemporaryHostsBytes(original, "github.com", "127.0.0.1").toString(), "127.0.0.1 localhost\n127.0.0.1 github.com\n");
  assert.throws(() => buildTemporaryHostsBytes(Buffer.from("127.0.0.1 github.com\n"), "github.com", "127.0.0.1"), /已含/);
  assert.throws(() => buildTemporaryHostsBytes(original, "api.github.com", "127.0.0.1"), /必须/);
});

test("trust plan 只删除本次 CA fingerprint", () => {
  const identity = { authority: "/tmp/authority.pem", sha1Fingerprint: "A".repeat(40) };
  const mac = tlsTrustCommandPlan("darwin", identity);
  assert.deepEqual(mac.remove.arguments.slice(-2), ["A".repeat(40), "/Library/Keychains/System.keychain"]);
  const windows = tlsTrustCommandPlan("win32", identity);
  assert.deepEqual(windows.remove.arguments, ["-user", "-delstore", "Root", "A".repeat(40)]);
  assert.throws(() => tlsTrustCommandPlan("linux", identity), /不支持/);
});

test("system adapter 实际按原字节恢复 hosts 并只移除本次 CA", async (context) => {
  const directory = await mkdtemp(join(tmpdir(), "narrant-system-adapter-"));
  context.after(() => rm(directory, { recursive: true, force: true }));
  const hostsPath = join(directory, "hosts");
  const original = Buffer.from("127.0.0.1 localhost\r\n# preserved\r\n");
  await writeFile(hostsPath, original);
  const commands = [];
  const system = createUpdateSmokeTlsSystemAdapter({ SystemRoot: "C:\\Windows", TEMP: directory }, {
    platform: "win32",
    hostsPath,
    temporaryDirectory: directory,
    runCommand: async (executable, arguments_) => { commands.push([executable, arguments_]); return ""; },
  });
  const identity = { authority: join(directory, "authority.pem"), sha1Fingerprint: "B".repeat(40) };
  await system.installCertificateAuthority(identity);
  await system.prepareHostnameRoute("github.com", "127.0.0.1");
  await system.routeHostname();
  assert.match((await readFile(hostsPath, "utf8")), /127\.0\.0\.1 github\.com\r\n$/);
  await system.restoreHostname();
  assert.equal((await readFile(hostsPath)).equals(original), true);
  await system.removeCertificateAuthority(identity);
  assert.deepEqual(commands.filter(([name]) => name === "certutil.exe").map(([, arguments_]) => arguments_), [
    ["-user", "-addstore", "-f", "Root", identity.authority],
    ["-user", "-delstore", "Root", identity.sha1Fingerprint],
  ]);
});

test("hosts 恢复失败时 CA cleanup 不删除可重试的原字节 backup", async (context) => {
  const directory = await mkdtemp(join(tmpdir(), "narrant-system-backup-"));
  context.after(() => rm(directory, { recursive: true, force: true }));
  const hostsPath = join(directory, "hosts");
  const original = Buffer.from("127.0.0.1 localhost\n");
  await writeFile(hostsPath, original);
  let installCount = 0;
  const system = createUpdateSmokeTlsSystemAdapter({ HOME: directory, TEMP: directory }, {
    platform: "darwin",
    hostsPath,
    temporaryDirectory: directory,
    runCommand: async (executable, arguments_) => {
      if (executable === "sudo" && arguments_.includes("install")) {
        installCount += 1;
        if (installCount === 2) throw new Error("restore install failed");
      }
      return "";
    },
  });
  const identity = { authority: join(directory, "authority.pem"), sha1Fingerprint: "C".repeat(40) };
  await system.installCertificateAuthority(identity);
  await system.prepareHostnameRoute("github.com", "127.0.0.1");
  await system.routeHostname();
  await assert.rejects(() => system.restoreHostname(), /restore install failed/);
  await system.removeCertificateAuthority(identity);
  const backup = join(directory, `narrant-hosts-${process.pid}.original`);
  assert.equal((await stat(backup)).isFile(), true);
  assert.equal((await readFile(backup)).equals(original), true);
});
