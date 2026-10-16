import { spawn } from "node:child_process";
import { Buffer } from "node:buffer";
import { chmod, readFile, rm, stat, writeFile } from "node:fs/promises";
import path from "node:path";
import process from "node:process";
import { clearTimeout, setTimeout } from "node:timers";

const OUTPUT_BOUND = 256 * 1024;
const HOSTS_BOUND = 1024 * 1024;
const SERVER_SCRIPT = path.resolve(import.meta.dirname, "update-smoke-tls-gate-server.mjs");

/** 向 exact child/process group 发信号。 */
function signalChild(child, signal) {
  if (!child.pid) return;
  if (process.platform !== "win32") try { process.kill(-child.pid, signal); } catch { /* 进程组可能已退出。 */ }
  try { child.kill(signal); } catch { /* exact child 可能已退出。 */ }
}

/** 无 shell 执行有界系统命令，超时后 TERM/KILL 并等待 close。 */
export function runBoundedCommand(executable, arguments_, options = {}) {
  return new Promise((resolve, reject) => {
    let finished = false;
    let reason;
    const child = spawn(executable, arguments_, {
      cwd: options.cwd,
      env: options.environment ?? {},
      shell: false,
      detached: process.platform !== "win32",
      windowsHide: true,
      stdio: ["ignore", "pipe", "pipe"],
    });
    const chunks = [];
    let size = 0;
    let hardKill;
    const finish = (error, value) => {
      if (finished) return;
      finished = true;
      clearTimeout(timeout);
      clearTimeout(hardKill);
      if (error) reject(error); else resolve(value);
    };
    const terminate = (why) => {
      if (reason) return;
      reason = why;
      signalChild(child, "SIGTERM");
      hardKill = setTimeout(() => signalChild(child, "SIGKILL"), 2_000);
    };
    const timeout = setTimeout(() => terminate("超时"), options.timeoutMilliseconds ?? 5 * 60 * 1000);
    for (const stream of [child.stdout, child.stderr]) stream.on("data", (chunk) => {
      size += chunk.length;
      if (size > (options.outputBound ?? OUTPUT_BOUND)) terminate("输出超过上限");
      else chunks.push(chunk);
    });
    child.once("error", (error) => finish(error));
    child.once("close", (code) => {
      const output = Buffer.concat(chunks).toString("utf8");
      if (reason) finish(new Error(`${path.basename(executable)} ${reason}`));
      else if (code !== 0) finish(new Error(`${path.basename(executable)} 失败（${code}）：${output.trim().slice(0, 2048)}`));
      else finish(undefined, output);
    });
  });
}

/** 只向系统工具传 runner 公共环境，不继承 Actions secret。 */
function selectCommandEnvironment(environment) {
  const names = [
    "PATH", "SystemRoot", "SystemDrive", "windir", "ComSpec", "PATHEXT", "PSModulePath", "OS",
    "HOME", "USERPROFILE", "USERNAME", "HOMEDRIVE", "HOMEPATH", "TEMP", "TMP", "TMPDIR",
  ];
  return Object.fromEntries(names.flatMap((name) => environment[name] === undefined ? [] : [[name, environment[name]]]));
}

/** 在不改变原字节的前提下追加唯一 github.com loopback 映射。 */
export function buildTemporaryHostsBytes(original, hostname, address) {
  if (!Buffer.isBuffer(original) || original.length > HOSTS_BOUND || original.includes(0)) throw new Error("runner hosts 文件无效");
  if (hostname !== "github.com" || address !== "127.0.0.1") throw new Error("hosts mapping 必须是 github.com→loopback");
  const body = original.toString("utf8");
  const existing = body.split(/\r?\n/).some((line) => {
    const fields = line.split("#", 1)[0].trim().split(/\s+/);
    return fields.slice(1).includes(hostname);
  });
  if (existing) throw new Error("runner hosts 已含 github.com，拒绝覆盖未知路由");
  const newline = body.includes("\r\n") ? "\r\n" : "\n";
  return Buffer.from(`${body}${body.length === 0 || body.endsWith("\n") ? "" : newline}${address} ${hostname}${newline}`);
}

/** 只为本次 CA 生成可逆 trust store 命令。 */
export function tlsTrustCommandPlan(platform, identity) {
  if (platform === "darwin") return {
    install: { executable: "sudo", arguments: ["-n", "security", "add-trusted-cert", "-d", "-r", "trustRoot", "-k", "/Library/Keychains/System.keychain", identity.authority] },
    remove: { executable: "sudo", arguments: ["-n", "security", "delete-certificate", "-Z", identity.sha1Fingerprint, "/Library/Keychains/System.keychain"] },
  };
  if (platform === "win32") return {
    install: { executable: "certutil.exe", arguments: ["-user", "-addstore", "-f", "Root", identity.authority] },
    remove: { executable: "certutil.exe", arguments: ["-user", "-delstore", "Root", identity.sha1Fingerprint] },
  };
  throw new Error(`不支持的 TLS trust 平台：${platform}`);
}

/** 解析 OpenSSL SHA-1 指纹。 */
function parseFingerprint(output) {
  const value = output.trim().split("=").at(-1)?.replaceAll(":", "").toUpperCase();
  if (!/^[0-9A-F]{40}$/.test(value ?? "")) throw new Error("临时 CA 指纹无效");
  return value;
}

/** 创建一天有效、SAN 只含 github.com 的 runner 临时 CA/leaf。 */
async function createTlsIdentity(config, command, environment) {
  if (config.hostname !== "github.com") throw new Error("临时 TLS identity hostname 无效");
  const authority = path.join(config.directory, "authority.pem");
  const authorityKey = path.join(config.directory, "authority.key");
  const certificate = path.join(config.directory, "leaf.pem");
  const key = path.join(config.directory, "leaf.key");
  const request = path.join(config.directory, "leaf.csr");
  const authorityConfig = path.join(config.directory, "authority.cnf");
  const leafConfig = path.join(config.directory, "leaf.cnf");
  await Promise.all([
    writeFile(authorityConfig, "[req]\nprompt=no\ndistinguished_name=dn\nx509_extensions=v3_ca\n[dn]\nCN=Narrant Update Smoke Temporary CA\n[v3_ca]\nbasicConstraints=critical,CA:true\nkeyUsage=critical,keyCertSign,cRLSign,digitalSignature\nsubjectKeyIdentifier=hash\nauthorityKeyIdentifier=keyid:always\n", { flag: "wx" }),
    writeFile(leafConfig, `[req]\nprompt=no\ndistinguished_name=dn\nreq_extensions=v3_req\n[dn]\nCN=${config.hostname}\n[v3_req]\nsubjectAltName=DNS:${config.hostname}\nkeyUsage=critical,digitalSignature,keyEncipherment\nextendedKeyUsage=serverAuth\n`, { flag: "wx" }),
  ]);
  await command("openssl", ["req", "-x509", "-newkey", "rsa:2048", "-sha256", "-nodes", "-days", "1", "-config", authorityConfig, "-keyout", authorityKey, "-out", authority], { environment });
  await command("openssl", ["req", "-new", "-newkey", "rsa:2048", "-sha256", "-nodes", "-config", leafConfig, "-keyout", key, "-out", request], { environment });
  await command("openssl", ["x509", "-req", "-in", request, "-CA", authority, "-CAkey", authorityKey, "-CAcreateserial", "-days", "1", "-sha256", "-extfile", leafConfig, "-extensions", "v3_req", "-out", certificate], { environment });
  const sha1Fingerprint = parseFingerprint(await command("openssl", ["x509", "-in", authority, "-noout", "-fingerprint", "-sha1"], { environment }));
  await Promise.all([chmod(authorityKey, 0o600), chmod(key, 0o600)]);
  return Object.freeze({ authority, certificate, key, sha1Fingerprint, directory: config.directory });
}

/** 启动 port 443 helper，并把 JSONL 事件变成有 deadline 的等待。 */
function startGateChild(config, environment, command) {
  const helperArguments = [SERVER_SCRIPT, "--hostname", config.hostname, "--certificate", config.identity.certificate, "--key", config.identity.key, "--routes", config.routesFile];
  const executable = process.platform === "win32" ? process.execPath : "sudo";
  const arguments_ = process.platform === "win32" ? helperArguments : ["-n", process.execPath, ...helperArguments];
  const child = spawn(executable, arguments_, {
    env: environment,
    shell: false,
    detached: process.platform !== "win32",
    windowsHide: true,
    stdio: ["pipe", "pipe", "pipe"],
  });
  let stdout = "";
  let stderr = "";
  let exit;
  const observed = new Set();
  const waiters = new Map();
  const rejectAll = (error) => {
    for (const waiter of waiters.values()) waiter.reject(error);
    waiters.clear();
  };
  child.stdout.setEncoding("utf8");
  child.stdout.on("data", (chunk) => {
    stdout += chunk;
    if (Buffer.byteLength(stdout) > OUTPUT_BOUND) {
      rejectAll(new Error("TLS helper stdout 超过上限"));
      signalChild(child, "SIGKILL");
      return;
    }
    for (;;) {
      const newline = stdout.indexOf("\n");
      if (newline < 0) break;
      const line = stdout.slice(0, newline);
      stdout = stdout.slice(newline + 1);
      try {
        const event = JSON.parse(line).event;
        const waiter = waiters.get(event);
        if (waiter) { waiters.delete(event); waiter.resolve(); } else observed.add(event);
      } catch { rejectAll(new Error("TLS helper 输出无效 JSONL")); }
    }
  });
  child.stderr.setEncoding("utf8");
  child.stderr.on("data", (chunk) => { if (Buffer.byteLength(stderr) < OUTPUT_BOUND) stderr += chunk; });
  child.once("error", (error) => { exit = { code: -1 }; rejectAll(error); });
  child.once("close", (code) => {
    exit = { code };
    if (code !== 0) rejectAll(new Error(`TLS helper 失败（${code}）：${stderr.trim().slice(0, 2048)}`));
  });
  const waitFor = (event, milliseconds = 30_000) => {
    if (observed.delete(event)) return Promise.resolve();
    if (exit) return Promise.reject(new Error(`TLS helper 在 ${event} 前退出`));
    return new Promise((resolve, reject) => {
      const timeout = setTimeout(() => { waiters.delete(event); reject(new Error(`TLS helper 等待 ${event} 超时`)); }, milliseconds);
      waiters.set(event, {
        resolve: () => { clearTimeout(timeout); resolve(); },
        reject: (error) => { clearTimeout(timeout); reject(error); },
      });
    });
  };
  const close = async () => {
    if (exit) {
      if (exit.code !== 0) throw new Error(`TLS helper 清理前异常退出（${exit.code}）`);
      return;
    }
    child.stdin.write(`${JSON.stringify({ command: "close" })}\n`);
    const deadline = Date.now() + 10_000;
    while (!exit && Date.now() < deadline) await new Promise((resolve) => setTimeout(resolve, 100));
    if (!exit) {
      if (process.platform === "win32") await command("taskkill.exe", ["/PID", String(child.pid), "/T", "/F"], { environment }).catch(() => {});
      else await command("sudo", ["-n", "/bin/kill", "-KILL", "--", `-${child.pid}`], { environment }).catch(() => {});
      throw new Error("TLS helper 无法在 deadline 内退出");
    }
    if (exit.code !== 0) throw new Error(`TLS helper 清理失败（${exit.code}）`);
  };
  return { waitFor, close };
}

/** 刷新 runner DNS cache；不支持的缓存服务不改变 hosts 主事实。 */
async function flushDns(platform, command, environment) {
  const plans = platform === "darwin"
    ? [["dscacheutil", ["-flushcache"]], ["sudo", ["-n", "killall", "-HUP", "mDNSResponder"]]]
    : [["ipconfig.exe", ["/flushdns"]]];
  for (const [executable, arguments_] of plans) await command(executable, arguments_, { environment }).catch(() => {});
}

/** 返回 macOS/Windows hosted runner 的可逆 TLS system adapter。 */
export function createUpdateSmokeTlsSystemAdapter(environment = process.env, dependencies = {}) {
  const command = dependencies.runCommand ?? runBoundedCommand;
  const commandEnvironment = selectCommandEnvironment(environment);
  const platform = dependencies.platform ?? process.platform;
  const hostsPath = dependencies.hostsPath ?? (platform === "win32"
    ? path.join(environment.SystemRoot ?? "C:\\Windows", "System32", "drivers", "etc", "hosts")
    : "/etc/hosts");
  let originalHosts;
  let originalPath;
  let temporaryPath;
  let trustPlan;
  return {
    /** 生成一次性 CA/leaf。 */
    createTlsIdentity: (config) => createTlsIdentity(config, command, commandEnvironment),
    /** 安装且仅安装本次 CA。 */
    async installCertificateAuthority(identity) {
      trustPlan = tlsTrustCommandPlan(platform, identity);
      try { await command(trustPlan.install.executable, trustPlan.install.arguments, { environment: commandEnvironment }); }
      catch (error) {
        await command(trustPlan.remove.executable, trustPlan.remove.arguments, { environment: commandEnvironment }).catch(() => {});
        trustPlan = undefined;
        throw error;
      }
    },
    /** 启动只服务两个本地已验证文件的 root port-443 helper。 */
    async startHttpsGate(config) {
      const routesFile = path.join(config.identity.directory, "routes.json");
      await writeFile(routesFile, `${JSON.stringify(config.routes)}\n`, { flag: "wx", mode: 0o600 });
      const control = startGateChild({ ...config, routesFile }, commandEnvironment, command);
      try { await control.waitFor("ready"); }
      catch (error) { await control.close().catch(() => {}); throw error; }
      return {
        waitForAllRequests: () => control.waitFor("all-requested", 8 * 60 * 1000),
        close: () => control.close(),
      };
    },
    /** 在任何 mutation 前备份原 hosts，并准备只含 exact mapping 的替换文件。 */
    async prepareHostnameRoute(hostname, address) {
      if (originalHosts) throw new Error("hosts 已由本次 adapter 修改");
      const original = await readFile(hostsPath);
      const temporary = buildTemporaryHostsBytes(original, hostname, address);
      const root = dependencies.temporaryDirectory ?? environment.RUNNER_TEMP ?? environment.TEMP ?? environment.TMPDIR;
      if (!root) throw new Error("runner 临时目录不可用");
      const backup = path.join(root, `narrant-hosts-${process.pid}.original`);
      const replacement = path.join(root, `narrant-hosts-${process.pid}.temporary`);
      await Promise.all([
        writeFile(backup, original, { flag: "wx", mode: 0o600 }),
        writeFile(replacement, temporary, { flag: "wx", mode: 0o600 }),
      ]);
      originalHosts = original;
      originalPath = backup;
      temporaryPath = replacement;
    },
    /** 应用已经 armed 的 hosts replacement；抛错后外层仍必须调用 restore。 */
    async routeHostname() {
      if (!originalHosts || !originalPath || !temporaryPath) throw new Error("hosts restore 尚未 armed");
      if (platform === "win32") await writeFile(hostsPath, await readFile(temporaryPath));
      else await command("sudo", ["-n", "install", "-m", "0644", temporaryPath, hostsPath], { environment: commandEnvironment });
      await flushDns(platform, command, commandEnvironment);
    },
    /** byte-for-byte 恢复 hosts，并回读确认。 */
    async restoreHostname() {
      if (!originalHosts || !originalPath) return;
      if (platform === "win32") await writeFile(hostsPath, originalHosts);
      else await command("sudo", ["-n", "install", "-m", "0644", originalPath, hostsPath], { environment: commandEnvironment });
      await flushDns(platform, command, commandEnvironment);
      if (!(await readFile(hostsPath)).equals(originalHosts)) throw new Error("hosts 未按原字节恢复");
      originalHosts = undefined;
    },
    /** 删除本次 CA；只有 hosts 已确认恢复时才删除备份。 */
    async removeCertificateAuthority() {
      if (trustPlan) await command(trustPlan.remove.executable, trustPlan.remove.arguments, { environment: commandEnvironment });
      trustPlan = undefined;
      if (!originalHosts) {
        for (const file of [originalPath, temporaryPath]) if (file && await stat(file).catch(() => null)) await rm(file, { force: true });
        originalPath = undefined;
        temporaryPath = undefined;
      }
    },
  };
}
