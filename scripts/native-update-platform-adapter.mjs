import { spawn } from "node:child_process";
import { Buffer } from "node:buffer";
import { createHash } from "node:crypto";
import { chmod, lstat, mkdir, mkdtemp, readFile, readdir, realpath, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import process from "node:process";
import { setTimeout } from "node:timers";

import { releaseAssetNames, verifyTauriUpdaterSignature } from "./release-assets.mjs";
import { FIXED_STABLE_ENDPOINT } from "./public-draft-assets.mjs";
import { createCandidateGateRoutes, withUpdateSmokeTlsGate } from "./update-smoke-tls-gate.mjs";
import { createUpdateSmokeTlsSystemAdapter, runBoundedCommand } from "./update-smoke-tls-system-adapter.mjs";
import { writeUpgradeUserSettings, registerUpgradeRepository, assertUpgradeUserData } from "./update-smoke-user-data.mjs";
import { selectSystemEnvironment as selectEnvironment } from "./update-smoke-environment.mjs";

const APP_IDENTIFIER = "com.cyunlab.narrant";
const MAC_CLOSE_HELPER = path.resolve(import.meta.dirname, "macos-close-narrant-window.swift");
const TARGETS = Object.freeze({
  "darwin-aarch64": Object.freeze({ platform: "darwin", arch: "arm64", installKind: "macos_app_archive" }),
  "windows-x86_64": Object.freeze({ platform: "win32", arch: "x64", installKind: "windows_nsis" }),
});
const MAX_METADATA_BYTES = 64 * 1024;
const MAX_DIAGNOSTICS_BYTES = 256 * 1024;

/** 计算完整 bytes 的 lowercase SHA-256。 */
function digest(bytes) { return createHash("sha256").update(bytes).digest("hex"); }

/** 有界轮询共用的短暂停顿。 */
function delay(milliseconds) { return new Promise((resolve) => setTimeout(resolve, milliseconds)); }

/** 为 bootstrap recovery fixture 生成严格高于当前 Stable 的下一个 patch 版本。 */
function nextPatchVersion(version) {
  const match = /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$/.exec(version ?? "");
  if (!match) throw new Error("bootstrap current version 不是稳定三段版本");
  return `${match[1]}.${match[2]}.${BigInt(match[3]) + 1n}`;
}

/** 写入字段均有效但签名/包必然失败的严格更高版本 staging fixture。 */
export async function writeBootstrapVerificationFixture(stagingRoot, options) {
  const generation = "generation-00000000-0000-4000-8000-000000000001";
  const root = path.join(stagingRoot, generation);
  const version = nextPatchVersion(options.currentVersion);
  await mkdir(root, { recursive: true });
  await Promise.all([
    writeFile(path.join(root, "package.bin"), "tampered-bootstrap-package", { flag: "wx" }),
    writeFile(path.join(root, "metadata.json"), `${JSON.stringify({
      schemaVersion: 2,
      version,
      signature: "invalid-bootstrap-verification-signature",
      target: options.target,
      installKind: options.installKind,
      releaseNotes: "bootstrap verification recovery fixture",
    })}\n`, { flag: "wx" }),
    writeFile(path.join(stagingRoot, "current"), generation, { flag: "wx" }),
  ]);
  return Object.freeze({ generation, version });
}

/** 读取有界 JSONL，确认 production startup recovery 发布 verification 警告。 */
async function hasVerificationRecoveryDiagnostic(diagnostics) {
  const bytes = await readFile(diagnostics).catch(() => undefined);
  if (!bytes) return false;
  if (bytes.length <= 0 || bytes.length > MAX_DIAGNOSTICS_BYTES) throw new Error("bootstrap recovery diagnostics 大小无效");
  const lines = bytes.toString("utf8").split("\n");
  if (lines.at(-1) !== "") return false;
  return lines.slice(0, -1).some((line) => {
    let record;
    try { record = JSON.parse(line); } catch { throw new Error("bootstrap recovery diagnostics JSONL 无效"); }
    return record?.event === "transition-warning" && record.category === "verification";
  });
}

/** 等待 pointer、generation 都清理且 verification recovery 诊断落盘。 */
export async function observeBootstrapRecovery(statePaths, generation, options = {}) {
  const deadline = Date.now() + (options.timeoutMilliseconds ?? 60_000);
  const interval = options.pollMilliseconds ?? 100;
  let remaining = "current pointer";
  while (Date.now() < deadline) {
    if (await lstat(path.join(statePaths.stagingRoot, "current")).catch(() => null)) {
      remaining = "current pointer";
    } else if (await lstat(path.join(statePaths.stagingRoot, generation)).catch(() => null)) {
      remaining = "generation";
    } else if (!await hasVerificationRecoveryDiagnostic(statePaths.diagnostics)) {
      remaining = "verification diagnostic";
    } else {
      return;
    }
    await delay(interval);
  }
  throw new Error(`bootstrap candidate recovery 未清理或确认 ${remaining}`);
}

/** 推导 Narrant 的 Tauri app-data/cache staging 边界。 */
export function platformStatePaths(target, environment) {
  if (target === "windows-x86_64") {
    if (!environment.APPDATA || !environment.LOCALAPPDATA) throw new Error("Windows app-data roots 不可用");
    return Object.freeze({
      dataRoot: path.win32.join(environment.APPDATA, APP_IDENTIFIER),
      cacheRoot: path.win32.join(environment.LOCALAPPDATA, APP_IDENTIFIER),
      stagingRoot: path.win32.join(environment.LOCALAPPDATA, APP_IDENTIFIER, "application-update"),
      diagnostics: path.win32.join(environment.APPDATA, APP_IDENTIFIER, "logs", "application-update.jsonl"),
    });
  }
  if (target === "darwin-aarch64") {
    if (!environment.HOME) throw new Error("macOS HOME 不可用");
    return Object.freeze({
      dataRoot: path.join(environment.HOME, "Library", "Application Support", APP_IDENTIFIER),
      cacheRoot: path.join(environment.HOME, "Library", "Caches", APP_IDENTIFIER),
      stagingRoot: path.join(environment.HOME, "Library", "Caches", APP_IDENTIFIER, "application-update"),
      diagnostics: path.join(environment.HOME, "Library", "Application Support", APP_IDENTIFIER, "logs", "application-update.jsonl"),
    });
  }
  throw new Error(`不支持的 target：${target ?? ""}`);
}

/** 从 generation pointer 验证 candidate metadata/package/signature identity。 */
export async function inspectDurableStaging(stagingRoot, expected) {
  const pointer = await readFile(path.join(stagingRoot, "current"), "utf8");
  if (pointer.length > 256 || !/^generation-[A-Za-z0-9_-]+$/.test(pointer)) throw new Error("staging current pointer 无效");
  const generation = pointer;
  const [metadataBytes, packageBytes] = await Promise.all([
    readFile(path.join(stagingRoot, generation, "metadata.json")),
    readFile(path.join(stagingRoot, generation, "package.bin")),
  ]);
  if (metadataBytes.length <= 0 || metadataBytes.length > MAX_METADATA_BYTES) throw new Error("staging metadata 大小无效");
  const metadata = JSON.parse(metadataBytes.toString("utf8"));
  if (
    metadata.schemaVersion !== 2 ||
    metadata.version !== expected.version ||
    metadata.signature !== expected.signature ||
    metadata.target !== expected.target ||
    metadata.installKind !== expected.installKind ||
    digest(packageBytes) !== expected.packageSha256
  ) {
    throw new Error("durable staging identity 不匹配");
  }
  return Object.freeze({ generation, metadata, packageSha256: digest(packageBytes) });
}

/** 从 macOS app bundle 读取 exact executable/version 并验证 arm64 签名与公证。 */
async function inspectMacApplication(application, expectedVersion, command, environment) {
  const canonical = await realpath(application);
  const plist = path.join(canonical, "Contents", "Info.plist");
  const executableName = (await command("/usr/libexec/PlistBuddy", ["-c", "Print :CFBundleExecutable", plist], { environment })).trim();
  const bundleIdentifier = (await command("/usr/libexec/PlistBuddy", ["-c", "Print :CFBundleIdentifier", plist], { environment })).trim();
  const version = (await command("/usr/libexec/PlistBuddy", ["-c", "Print :CFBundleShortVersionString", plist], { environment })).trim();
  if (bundleIdentifier !== APP_IDENTIFIER || version !== expectedVersion) throw new Error("macOS installed app identity 不匹配");
  const executable = await realpath(path.join(canonical, "Contents", "MacOS", executableName));
  const architectures = await command("lipo", ["-archs", executable], { environment });
  if (architectures.trim() !== "arm64") throw new Error("macOS Narrant 必须是 thin arm64 app");
  await command("codesign", ["--verify", "--deep", "--strict", "--verbose=2", canonical], { environment });
  await command("xcrun", ["stapler", "validate", canonical], { environment });
  await command("spctl", ["--assess", "--type", "execute", canonical], { environment });
  return Object.freeze({ root: canonical, application: canonical, executable, version });
}

/** 接受 NSIS registry 常见的单对外层引号，并严格规范化绝对安装路径。 */
export function normalizeWindowsInstallLocation(value) {
  if (
    typeof value !== "string" ||
    !value ||
    value.trim() !== value ||
    /[\0\r\n]/.test(value)
  ) {
    throw new Error("Windows InstallLocation 格式无效");
  }
  const startsQuoted = value.startsWith('"');
  const endsQuoted = value.endsWith('"');
  if (startsQuoted !== endsQuoted) {
    throw new Error("Windows InstallLocation 引号不配对");
  }
  const normalized = startsQuoted ? value.slice(1, -1) : value;
  if (
    !normalized ||
    normalized.trim() !== normalized ||
    normalized.includes('"') ||
    !path.win32.isAbsolute(normalized)
  ) {
    throw new Error("Windows InstallLocation 不是有效绝对路径");
  }
  return normalized;
}

/** 从唯一 HKCU uninstall record 定位 current-user Narrant 安装。 */
async function inspectWindowsInstallation(expectedVersion, command, environment) {
  const script = "$ErrorActionPreference='Stop';$cu=@(Get-ItemProperty 'HKCU:\\Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\*'|Where-Object {$_.DisplayName -eq 'Narrant'});$lm=@(Get-ItemProperty 'HKLM:\\Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\*','HKLM:\\Software\\WOW6432Node\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\*' -ErrorAction SilentlyContinue|Where-Object {$_.DisplayName -eq 'Narrant'});if($cu.Count -ne 1 -or $lm.Count -ne 0){throw 'expected one HKCU Narrant install'};[Console]::Out.Write(($cu[0]|Select-Object InstallLocation,DisplayVersion,WindowsInstaller,UninstallString|ConvertTo-Json -Compress))";
  const record = JSON.parse(await command("powershell.exe", ["-NoLogo", "-NoProfile", "-NonInteractive", "-Command", script], { environment }));
  if (record.DisplayVersion !== expectedVersion || record.WindowsInstaller === 1) throw new Error("Windows Narrant version/MSI identity 无效");
  const root = await realpath(normalizeWindowsInstallLocation(record.InstallLocation));
  if (!root.toLowerCase().startsWith((environment.LOCALAPPDATA ?? environment.USERPROFILE).toLowerCase())) {
    throw new Error("Windows Narrant 不是 current-user installation");
  }
  const entries = await readdir(root);
  const executables = entries.filter((name) => name.toLowerCase().endsWith(".exe") && !name.toLowerCase().includes("uninstall"));
  const uninstallers = entries.filter((name) => name.toLowerCase().endsWith(".exe") && name.toLowerCase().includes("uninstall"));
  if (executables.length !== 1 || uninstallers.length !== 1) throw new Error("Windows Narrant executable/uninstaller 不唯一");
  return Object.freeze({ root, executable: path.win32.join(root, executables[0]), uninstaller: path.win32.join(root, uninstallers[0]), version: record.DisplayVersion });
}

/** 证明安装二进制包含 shipped endpoint 和 exact updater trust key。 */
async function inspectTrustedConfiguration(installation, publicKey) {
  const executable = await readFile(installation.executable);
  if (!executable.includes(Buffer.from(FIXED_STABLE_ENDPOINT)) || !executable.includes(Buffer.from(publicKey))) {
    throw new Error("installed Narrant 缺少固定 updater trust configuration");
  }
  return true;
}

/** 在当前平台枚举 exact installed executable PID。 */
async function findInstalledPids(target, installation, command, environment) {
  if (target === "windows-x86_64") {
    const escaped = installation.executable.replaceAll("'", "''");
    const script = `$p=@(Get-Process|Where-Object {try{[IO.Path]::GetFullPath($_.Path) -ieq '${escaped}'}catch{$false}}|Select-Object -ExpandProperty Id);[Console]::Out.Write(($p -join [Environment]::NewLine))`;
    const output = await command("powershell.exe", ["-NoLogo", "-NoProfile", "-NonInteractive", "-Command", script], { environment });
    return output.split(/\r?\n/).filter((value) => /^\d+$/.test(value.trim())).map(Number);
  }
  const output = await command("ps", ["-axo", "pid=,command="], { environment, outputBound: 4 * 1024 * 1024 });
  return output.split(/\r?\n/).flatMap((line) => {
    const match = /^\s*(\d+)\s+(.+?)\s*$/.exec(line);
    return match && (match[2] === installation.executable || match[2].startsWith(`${installation.executable} `)) ? [Number(match[1])] : [];
  });
}

/** 启动 exact installed app，等待唯一进程完成启动并显示原生窗口。 */
export async function launchInstalled(target, installation, command, environment, closeHelper) {
  if ((await findInstalledPids(target, installation, command, environment)).length) throw new Error("Narrant 启动前已有同安装来源进程");
  if (target === "darwin-aarch64") await command("open", ["-n", "-a", installation.application], { environment });
  else spawn(installation.executable, [], { env: environment, shell: false, windowsHide: false, stdio: "ignore", detached: false });
  const deadline = Date.now() + 60_000;
  while (Date.now() < deadline) {
    const pids = await findInstalledPids(target, installation, command, environment);
    if (pids.length === 1) {
      const ready = target === "windows-x86_64"
        ? await command("powershell.exe", ["-NoLogo", "-NoProfile", "-NonInteractive", "-Command",
          `$p=Get-Process -Id ${pids[0]} -ErrorAction Stop;if($p.MainWindowHandle -ne 0 -and $p.Responding){[Console]::Out.Write('ready')}`], { environment })
        : await command(closeHelper, [String(pids[0]), "ready"], { environment });
      if (ready.trim() === "ready") return Object.freeze({ pid: pids[0], installation });
    }
    if (pids.length > 1) throw new Error("exact Narrant process 不唯一");
    await delay(250);
  }
  throw new Error("installed Narrant 未在 deadline 内启动");
}

/** 关闭唯一原生窗口，从而触发 production normal-exit shutdown/install。 */
async function requestNormalClose(target, launch, closeHelper, command, environment) {
  if (target === "windows-x86_64") {
    const script = `$p=Get-Process -Id ${launch.pid} -ErrorAction Stop;if(-not $p.CloseMainWindow()){throw 'Narrant main window did not accept close'}`;
    await command("powershell.exe", ["-NoLogo", "-NoProfile", "-NonInteractive", "-Command", script], { environment });
  } else {
    await command(closeHelper, [String(launch.pid)], { environment, timeoutMilliseconds: 30_000 });
  }
}

/** 等待 exact app PID 全部退出。 */
async function waitForNoInstalledProcess(target, installation, command, environment, milliseconds = 4 * 60 * 1000) {
  const deadline = Date.now() + milliseconds;
  while (Date.now() < deadline) {
    if ((await findInstalledPids(target, installation, command, environment)).length === 0) return;
    await delay(500);
  }
  throw new Error("Narrant 未在 deadline 内退出");
}

/** 把 bootstrap candidate installer 安装到 runner。 */
async function installBootstrapCandidate(target, installer, version, temporary, command, environment) {
  if (target === "windows-x86_64") {
    await command(installer, ["/S"], { environment });
    return inspectWindowsInstallation(version, command, environment);
  }
  const mount = path.join(temporary, "dmg-mount");
  const installRoot = path.join(temporary, "install");
  await Promise.all([mkdir(mount), mkdir(installRoot)]);
  let attached = false;
  try {
    await command("codesign", ["--verify", "--strict", "--verbose=2", installer], { environment });
    await command("xcrun", ["stapler", "validate", installer], { environment });
    await command("spctl", ["--assess", "--type", "open", "--context", "context:primary-signature", installer], { environment });
    await command("hdiutil", ["attach", installer, "-readonly", "-nobrowse", "-mountpoint", mount], { environment });
    attached = true;
    const apps = (await readdir(mount)).filter((name) => name.endsWith(".app"));
    if (apps.length !== 1 || apps[0] !== "Narrant.app") throw new Error("bootstrap DMG app layout 无效");
    await command("ditto", [path.join(mount, apps[0]), path.join(installRoot, apps[0])], { environment });
  } finally {
    if (attached) await command("hdiutil", ["detach", mount], { environment }).catch(() => {});
  }
  return inspectMacApplication(path.join(installRoot, "Narrant.app"), version, command, environment);
}

/** 把 previous Stable updater package 安装到隔离 runner location。 */
async function installPreviousStable(target, packagePath, version, temporary, command, environment) {
  if (target === "windows-x86_64") {
    await command(packagePath, ["/S"], { environment });
    return inspectWindowsInstallation(version, command, environment);
  }
  const installRoot = path.join(temporary, "install");
  await mkdir(installRoot);
  await command("tar", ["-xzf", packagePath, "-C", installRoot], { environment, outputBound: 4 * 1024 * 1024 });
  const apps = (await readdir(installRoot)).filter((name) => name.endsWith(".app"));
  if (apps.length !== 1) throw new Error("previous Stable archive app 不唯一");
  return inspectMacApplication(path.join(installRoot, apps[0]), version, command, environment);
}

/** 编译无需 Accessibility 权限的 macOS exact-application terminate helper。 */
async function compileMacCloseHelper(temporary, command, environment) {
  const output = path.join(temporary, "close-narrant-window");
  await command("swiftc", ["-parse-as-library", "-framework", "AppKit", MAC_CLOSE_HELPER, "-o", output], { environment });
  await chmod(output, 0o700);
  return output;
}

/** 创建 normal/bootstrap 共用的真实 macOS/Windows 平台适配器。 */
export function createNativeUpdatePlatformAdapter(target, environment = process.env, dependencies = {}) {
  const contract = TARGETS[target];
  if (!contract) throw new Error(`不支持的 native target：${target ?? ""}`);
  const command = dependencies.runCommand ?? runBoundedCommand;
  const commandEnvironment = selectEnvironment(environment);
  const tlsSystemAdapter = dependencies.tlsSystemAdapter ?? createUpdateSmokeTlsSystemAdapter(environment);
  let temporary;
  let installation;
  let closeHelper;
  let statePaths;
  let ownsState = false;
  let repositoryRoot;

  /** 初始化干净 runner 与 staging roots。 */
  async function initialize() {
    if (environment.GITHUB_ACTIONS !== "true" || environment.RUNNER_ENVIRONMENT !== "github-hosted") throw new Error("正式包升级验收仅允许 GitHub-hosted 临时机器");
    if (process.platform !== contract.platform || process.arch !== contract.arch) throw new Error("hosted runner 与 native target 不匹配");
    temporary = await mkdtemp(path.join(tmpdir(), "narrant-native-update-"));
    statePaths = platformStatePaths(target, commandEnvironment);
    for (const root of [statePaths.dataRoot, statePaths.cacheRoot]) {
      if (await lstat(root).catch(() => null)) throw new Error("native smoke runner 含旧 Narrant state");
    }
    ownsState = true;
    if (target === "darwin-aarch64") closeHelper = await compileMacCloseHelper(temporary, command, commandEnvironment);
  }

  /** 准备专用临时项目与设置，实际应用继续使用正式数据路径。 */
  async function prepareUserData() {
    repositoryRoot = path.join(temporary, "upgrade-project");
    await command("git", ["init", "--quiet", repositoryRoot], { environment: commandEnvironment });
    repositoryRoot = await realpath(repositoryRoot);
    await writeUpgradeUserSettings(statePaths.dataRoot);
  }

  /** 安装 previous Stable 并验证签名、版本、配置。 */
  async function installBaseline(options) {
    const [packageBytes, signature] = await Promise.all([readFile(options.baselinePackage), readFile(options.baselineSignature, "utf8")]);
    verifyTauriUpdaterSignature(packageBytes, signature, options.previousReleaseSigningPublicKey);
    installation = await installPreviousStable(target, options.baselinePackage, options.previous.version, temporary, command, commandEnvironment);
    await inspectTrustedConfiguration(installation, options.releaseSigningPublicKey);
    return installation;
  }

  /** 安装 candidate installer，供 first-updater bootstrap 使用。 */
  async function installBootstrap(options) {
    if (target === "windows-x86_64") {
      const names = releaseAssetNames(options.version);
      const updater = path.join(options.candidateDirectory, names.updater.windows);
      const [installerBytes, updaterBytes, updaterSignature] = await Promise.all([
        readFile(options.candidateInstaller), readFile(updater), readFile(`${updater}.sig`, "utf8"),
      ]);
      if (digest(installerBytes) !== digest(updaterBytes)) throw new Error("bootstrap Windows installer 与已签 updater bytes 不同");
      verifyTauriUpdaterSignature(installerBytes, updaterSignature, options.releaseSigningPublicKey);
    }
    installation = await installBootstrapCandidate(target, options.candidateInstaller, options.version, temporary, command, commandEnvironment);
    await inspectTrustedConfiguration(installation, options.candidateUpdaterPublicKey);
    return installation;
  }

  /** 在固定 GitHub TLS gate 内运行旧应用真实下载生命周期。 */
  async function withCandidateGate(options, action) {
    const routes = createCandidateGateRoutes({
      target, tag: options.publicTag, version: options.version,
      directory: options.candidateDirectory, manifest: options.manifest,
    });
    return withUpdateSmokeTlsGate({ endpoint: FIXED_STABLE_ENDPOINT, tag: options.publicTag, routes }, action, tlsSystemAdapter);
  }

  /** 等待当前 candidate 进入 durable staging。 */
  async function waitForStaging(options) {
    const names = releaseAssetNames(options.version);
    const packageName = target === "darwin-aarch64" ? names.updater.darwin : names.updater.windows;
    const signature = await readFile(path.join(options.candidateDirectory, `${packageName}.sig`), "utf8");
    const packageSha256 = digest(await readFile(path.join(options.candidateDirectory, packageName)));
    const deadline = Date.now() + 8 * 60 * 1000;
    let last;
    while (Date.now() < deadline) {
      try {
        return await inspectDurableStaging(statePaths.stagingRoot, {
          version: options.version, signature, target, installKind: contract.installKind, packageSha256,
        });
      } catch (error) { last = error; await delay(500); }
    }
    throw new Error(`candidate 未达到 durable staging：${last?.message ?? "unknown"}`);
  }

  /** 轮询同一安装位置的 candidate version。 */
  async function inspectUpdated(options, baseline) {
    const deadline = Date.now() + 4 * 60 * 1000;
    let last;
    while (Date.now() < deadline) {
      try {
        const updated = target === "windows-x86_64"
          ? await inspectWindowsInstallation(options.version, command, commandEnvironment)
          : await inspectMacApplication(baseline.application, options.version, command, commandEnvironment);
        if (updated.root.toLowerCase() !== baseline.root.toLowerCase() || updated.executable.toLowerCase() !== baseline.executable.toLowerCase()) {
          throw new Error("candidate 未替换同一安装位置");
        }
        installation = updated;
        return updated;
      } catch (error) { last = error; await delay(1_000); }
    }
    throw new Error(`candidate installation 未就绪：${last?.message ?? "unknown"}`);
  }

  /** 注入损坏 staging，并证明 candidate 启动恢复时会清除。 */
  async function proveBootstrapRecovery(options) {
    const fixture = await writeBootstrapVerificationFixture(statePaths.stagingRoot, {
      currentVersion: options.version,
      target,
      installKind: contract.installKind,
    });
    const launch = await launchInstalled(target, installation, command, commandEnvironment, closeHelper);
    await observeBootstrapRecovery(statePaths, fixture.generation);
    return launch;
  }

  /** 失败路径终止进程，正常路径卸载/删除本次 runner state。 */
  async function cleanup() {
    const errors = [];
    // 启动未就绪便失败时也按安装路径回收，不能只清理成功返回的 launch。
    if (installation) {
      const pids = await findInstalledPids(target, installation, command, commandEnvironment).catch((error) => { errors.push(error); return []; });
      for (const pid of pids) {
        if (target === "windows-x86_64") await command("taskkill.exe", ["/PID", String(pid), "/T", "/F"], { environment: commandEnvironment }).catch((error) => errors.push(error));
        else try { process.kill(pid, "SIGKILL"); } catch { /* 进程可能已自然退出。 */ }
      }
      await waitForNoInstalledProcess(target, installation, command, commandEnvironment, 30_000).catch((error) => errors.push(error));
    }
    if (target === "windows-x86_64" && installation?.uninstaller && await lstat(installation.uninstaller).catch(() => null)) {
      await command(installation.uninstaller, ["/S"], { environment: commandEnvironment }).catch((error) => errors.push(error));
    }
    const roots = [...(ownsState ? [statePaths.dataRoot, statePaths.cacheRoot] : []), temporary].filter(Boolean);
    for (const root of roots) await rm(root, { recursive: true, force: true }).catch((error) => errors.push(error));
    if (errors.length === 1) throw errors[0];
    if (errors.length > 1) throw new AggregateError(errors, "native platform cleanup 失败");
  }

  return Object.freeze({
    target,
    runner: target === "darwin-aarch64" ? { os: "macos", arch: "aarch64", image: "macos-15" } : { os: "windows", arch: "x86_64", image: "windows-2022" },
    platform: target === "darwin-aarch64"
      ? { packageKind: "app-tar-gz", installerKind: "dmg", installScope: "user", codeSigning: "verified", notarization: "verified" }
      : { packageKind: "nsis-exe", installerKind: "nsis-exe", installScope: "current-user", registryRoot: "HKCU", msiPresent: false, authenticode: "not-required" },
    initialize,
    installBaseline,
    prepareUserData,
    /** 使用旧版已初始化的 schema 登记测试项目。 */
    registerUserRepository: () => registerUpgradeRepository(statePaths.dataRoot, repositoryRoot),
    /** 候选正常退出后核对真实持久化数据。 */
    assertUserData: () => assertUpgradeUserData(statePaths.dataRoot, repositoryRoot),
    installBootstrap,
    withCandidateGate,
    /** 原生窗口已就绪才报告启动成功。 */
    launch: () => launchInstalled(target, installation, command, commandEnvironment, closeHelper),
    waitForStaging,
    requestNormalClose: (launch) => requestNormalClose(target, launch, closeHelper, command, commandEnvironment),
    waitForExit: (launch) => waitForNoInstalledProcess(target, launch.installation, command, commandEnvironment),
    inspectUpdated,
    assertNotRelaunched: async () => {
      const deadline = Date.now() + 5_000;
      while (Date.now() < deadline) {
        if ((await findInstalledPids(target, installation, command, commandEnvironment)).length) throw new Error("normal exit 自动重启了 Narrant");
        await delay(500);
      }
    },
    inspectTrustedConfiguration: (publicKey) => inspectTrustedConfiguration(installation, publicKey),
    proveBootstrapRecovery,
    cleanup,
  });
}
