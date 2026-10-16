import { spawn } from "node:child_process";
import { Buffer } from "node:buffer";
import { chmod, stat, writeFile } from "node:fs/promises";
import path from "node:path";
import process from "node:process";
import { clearTimeout, setTimeout } from "node:timers";

const OUTPUT_BOUND = 256 * 1024;

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
      try { options.onOutput?.(chunk); }
      catch { terminate("日志写入失败"); }
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
export function selectCommandEnvironment(environment) {
  const names = [
    "PATH", "SystemRoot", "SystemDrive", "windir", "ComSpec", "PATHEXT", "PSModulePath", "OS",
    "HOME", "USERPROFILE", "USERNAME", "HOMEDRIVE", "HOMEPATH", "LOCALAPPDATA", "APPDATA",
    "ProgramData", "ProgramFiles", "ProgramFiles(x86)", "TEMP", "TMP", "TMPDIR", "LANG", "LC_ALL",
  ];
  return Object.fromEntries(names.flatMap((name) => environment[name] === undefined ? [] : [[name, environment[name]]]));
}

/** 优先使用 PATH；Windows 复用现有 Git 自带的 OpenSSL，不另装工具或修改机器 PATH。 */
export async function resolveOpenSsl(environment = process.env) {
  const commandEnvironment = selectCommandEnvironment(environment);
  try {
    await runBoundedCommand("openssl", ["version"], { environment: commandEnvironment });
    return "openssl";
  } catch (error) {
    if (process.platform !== "win32" || error.code !== "ENOENT") throw error;
    const bundled = path.join(environment.ProgramFiles ?? "C:\\Program Files", "Git/usr/bin/openssl.exe");
    if (!(await stat(bundled).catch(() => undefined))?.isFile()) throw new Error("需要 PATH 中的 OpenSSL 或现有 Git 自带的 openssl.exe");
    await runBoundedCommand(bundled, ["version"], { environment: commandEnvironment });
    return bundled;
  }
}

/** 创建一天有效、SAN 只含 updates.cyunlab.com 的 runner 临时 CA/leaf。 */
export async function createTlsIdentity(config, environment = process.env) {
  const command = runBoundedCommand;
  environment = selectCommandEnvironment(environment);
  const openssl = await resolveOpenSsl(environment);
  if (config.hostname !== "updates.cyunlab.com") throw new Error("临时 TLS identity hostname 无效");
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
  await command(openssl, ["req", "-x509", "-newkey", "rsa:2048", "-sha256", "-nodes", "-days", "1", "-config", authorityConfig, "-keyout", authorityKey, "-out", authority], { environment });
  await command(openssl, ["req", "-new", "-newkey", "rsa:2048", "-sha256", "-nodes", "-config", leafConfig, "-keyout", key, "-out", request], { environment });
  await command(openssl, ["x509", "-req", "-in", request, "-CA", authority, "-CAkey", authorityKey, "-CAcreateserial", "-days", "1", "-sha256", "-extfile", leafConfig, "-extensions", "v3_req", "-out", certificate], { environment });
  await Promise.all([chmod(authorityKey, 0o600), chmod(key, 0o600)]);
  return Object.freeze({ authority, certificate, key, directory: config.directory });
}
