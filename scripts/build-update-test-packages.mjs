import { createHash } from "node:crypto";
import { Buffer } from "node:buffer";
import { appendFileSync } from "node:fs";
import { cp, mkdir, readFile, readdir, rm, writeFile } from "node:fs/promises";
import path from "node:path";
import process from "node:process";
import { pathToFileURL } from "node:url";

import { runBoundedCommand } from "./update-smoke-tools.mjs";
import { UPDATE_TEST_PROFILE } from "./update-test-profile.mjs";
import { FIXED_STABLE_ENDPOINT } from "./public-draft-assets.mjs";
import { verifyTauriUpdaterSignature } from "./release-assets.mjs";

const ROOT = path.resolve(import.meta.dirname, "..");
const CLI = path.join(ROOT, "node_modules/@tauri-apps/cli/tauri.js");
const credentialFiles = ["application_settings/credential.rs", "application_settings/mod.rs", "github/auth/credential_store.rs", "github/auth/mod.rs"];

/** 用实际文件字节记录源码叠加与包身份。 */
function sha256(bytes) { return createHash("sha256").update(bytes).digest("hex"); }

/** 仅向源码叠加测试身份、内存凭据和观察入口，不更改各版本的更新器实现。 */
export async function prepareUpdateTestSource(directory, version, publicKey) {
  const cargo = path.join(directory, "src-tauri/Cargo.toml");
  let manifest = await readFile(cargo, "utf8");
  if (!manifest.includes("test-isolation =")) {
    manifest = manifest.replace("[features]", '[features]\ntest-isolation = []\nupgrade-smoke = ["test-isolation"]')
      .replace('wdio = [', 'wdio = ["test-isolation", ');
  }
  if (!manifest.includes('upgrade-smoke = ["test-isolation"]')) throw new Error("未知的升级测试 feature 声明");
  await writeFile(cargo, manifest);
  for (const name of credentialFiles) {
    const file = path.join(directory, "src-tauri/src", name);
    await writeFile(file, (await readFile(file, "utf8")).replaceAll('feature = "wdio"', 'feature = "test-isolation"'));
  }
  const bootstrapPath = path.join(directory, "src-tauri/src/tauri_app/bootstrap.rs");
  let bootstrap = await readFile(bootstrapPath, "utf8");
  if (!bootstrap.includes("let context = tauri::generate_context!();")) {
    bootstrap = bootstrap.replace("pub fn run() {", "pub fn run() {\n    let context = tauri::generate_context!();")
      .replace(".build(tauri::generate_context!())", ".build(context)");
  }
  if (!bootstrap.includes(UPDATE_TEST_PROFILE.identifier)) {
    bootstrap = bootstrap.replace("let context = tauri::generate_context!();",
      `let context = tauri::generate_context!();\n    #[cfg(feature = "upgrade-smoke")]\n    assert_eq!(context.config().identifier, "${UPDATE_TEST_PROFILE.identifier}", "Upgrade tests require an isolated identity");`);
  }
  const authStart = bootstrap.indexOf("let auth =");
  const authEnd = bootstrap.indexOf("let store =", authStart);
  if (authStart < 0 || authEnd < 0) throw new Error("旧版凭据装配边界已变化，需要审查测试叠加");
  bootstrap = bootstrap.slice(0, authStart) + bootstrap.slice(authStart, authEnd).replaceAll('feature = "wdio"', 'feature = "test-isolation"') + bootstrap.slice(authEnd);
  if (!bootstrap.includes("super::upgrade_smoke::prepare")) {
    const anchor = "let application_settings = ApplicationSettingsApplication::new(settings_path.clone());";
    if (!bootstrap.includes(anchor)) throw new Error("旧版设置装配边界已变化，需要审查测试叠加");
    bootstrap = bootstrap.replace(anchor, `${anchor}\n            #[cfg(feature = "upgrade-smoke")]\n            super::upgrade_smoke::prepare(app, &application_settings)?;`);
  }
  await writeFile(bootstrapPath, bootstrap);
  const modulePath = path.join(directory, "src-tauri/src/tauri_app/mod.rs");
  let module = await readFile(modulePath, "utf8");
  if (!module.includes("mod upgrade_smoke;")) module += '\n#[cfg(feature = "upgrade-smoke")]\nmod upgrade_smoke;\n';
  await writeFile(modulePath, module);
  await cp(path.join(ROOT, "src-tauri/src/tauri_app/upgrade_smoke.rs"), path.join(directory, "src-tauri/src/tauri_app/upgrade_smoke.rs"));
  const config = JSON.parse(await readFile(path.join(ROOT, "src-tauri/tauri.upgrade-test.conf.json"), "utf8"));
  config.version = version;
  config.plugins = { updater: { pubkey: publicKey } };
  config.build = { runner: path.join(ROOT, "scripts", process.platform === "win32" ? "cargo-agent.cmd" : "cargo-agent.sh") };
  await writeFile(path.join(directory, "src-tauri/tauri.upgrade-test.conf.json"), `${JSON.stringify(config, null, 2)}\n`);
}

/** 递归查找本次构建唯一的 updater 制品。 */
async function findPackage(directory, suffix) {
  const found = [];
  for (const entry of await readdir(directory, { withFileTypes: true })) {
    const file = path.join(directory, entry.name);
    if (entry.isDirectory()) found.push(...await findPackage(file, suffix));
    else if (entry.isFile() && entry.name.endsWith(suffix)) found.push(file);
  }
  return found;
}

/** 从精确提交导出独立源码树，保留完整测试叠加 diff。 */
async function exportSource(sha, directory, environment) {
  if (!/^[0-9a-f]{40}$/.test(sha)) throw new Error("测试构建必须指定完整源码 SHA");
  await mkdir(directory);
  const archive = `${directory}.tar`;
  await runBoundedCommand("git", ["archive", "--format=tar", `--output=${archive}`, sha], { cwd: ROOT, environment });
  await runBoundedCommand("tar", ["-xf", archive, "-C", directory], { environment });
  await rm(archive);
  await runBoundedCommand("git", ["init", "--quiet", "--template="], { cwd: directory, environment });
  await runBoundedCommand("git", ["add", "-A"], { cwd: directory, environment });
}

/** 构建一对有独立签名和身份的真实安装包；不上传、不发布、不使用生产私钥。 */
export async function buildUpdateTestPackages(options, environment = process.env) {
  const target = process.platform === "darwin" && process.arch === "arm64" ? "darwin-aarch64"
    : process.platform === "win32" && process.arch === "x64" ? "windows-x86_64" : undefined;
  if (!target) throw new Error("升级测试仅支持 macOS ARM64 与 Windows x64");
  const output = path.resolve(options.output);
  await mkdir(output); // 排他创建；拒绝覆盖上次运行、签名或证据。
  const buildEnvironment = { ...environment };
  delete buildEnvironment.CARGO_TARGET_DIR;
  for (const name of Object.keys(buildEnvironment)) if (/^(APPLE_|TAURI_SIGNING_|NARRANT_UPDATER_|NARRANT_RELEASE_SIGNING_)/.test(name)) delete buildEnvironment[name];
  buildEnvironment.NARRANT_CARGO_LOCK_FILE ??= path.join((await runBoundedCommand("git", ["rev-parse", "--path-format=absolute", "--git-common-dir"], { cwd: ROOT, environment: buildEnvironment })).trim(), "narrant-cargo-agent.lock");
  const privateKeyPath = path.join(output, "test-only.key");
  const password = "narrant-upgrade-test";
  await runBoundedCommand(process.execPath, [CLI, "signer", "generate", "--ci", "--password", password, "--write-keys", privateKeyPath], { environment: buildEnvironment });
  const publicKey = (await readFile(`${privateKeyPath}.pub`, "utf8")).trim();
  Object.assign(buildEnvironment, {
    CI: "true", NARRANT_UPDATER_ENDPOINT: FIXED_STABLE_ENDPOINT, NARRANT_UPDATER_PUBLIC_KEY: publicKey,
    TAURI_SIGNING_PRIVATE_KEY: await readFile(privateKeyPath, "utf8"), TAURI_SIGNING_PRIVATE_KEY_PASSWORD: password,
    APPLE_SIGNING_IDENTITY: "-",
  });
  const recipe = await Promise.all([
    readFile(path.join(ROOT, "scripts/build-update-test-packages.mjs")),
    readFile(path.join(ROOT, "src-tauri/src/tauri_app/upgrade_smoke.rs")),
    readFile(path.join(ROOT, "src-tauri/tauri.upgrade-test.conf.json")),
    readFile(path.join(ROOT, "scripts/update-test-profile.mjs")),
  ]);
  const receipt = { schemaVersion: 1, profile: UPDATE_TEST_PROFILE, target, publicKey, recipeSha256: sha256(Buffer.concat(recipe)), builds: {} };
  try {
    for (const stage of ["baseline", "candidate"]) {
      const sourceSha = options[`${stage}Sha`];
      const version = options[`${stage}Version`];
      if (!/^\d+\.\d+\.\d+$/.test(version ?? "")) throw new Error("升级测试版本无效");
      const source = path.join(output, `${stage}-source`);
      await exportSource(sourceSha, source, buildEnvironment);
      const sourceVersion = JSON.parse(await readFile(path.join(source, "src-tauri/tauri.conf.json"), "utf8")).version;
      if (stage === "baseline" && sourceVersion !== version) throw new Error("旧版源码版本与 Stable 版本不一致");
      await prepareUpdateTestSource(source, version, publicKey);
      await runBoundedCommand("git", ["add", "-N", "."], { cwd: source, environment: buildEnvironment });
      const overlay = await runBoundedCommand("git", ["diff", "--binary"], { cwd: source, environment: buildEnvironment, outputBound: 2 * 1024 * 1024 });
      await writeFile(path.join(output, `${stage}-overlay.patch`), overlay);
      const pnpm = environment.npm_execpath;
      if (!pnpm) throw new Error("请从 pnpm 脚本运行隔离包构建");
      process.stdout.write(`Building ${stage} ${version} from ${sourceSha}\n`);
      await runBoundedCommand(process.execPath, [pnpm, "install", "--frozen-lockfile"], { cwd: source, environment: buildEnvironment, timeoutMilliseconds: 15 * 60_000 });
      const logPath = path.join(output, `${stage}-build.log`);
      await runBoundedCommand(process.execPath, [CLI, "build", "--features", "upgrade-smoke", "--config", "src-tauri/tauri.upgrade-test.conf.json", "--bundles", target === "darwin-aarch64" ? "app" : "nsis"], {
        cwd: source, environment: buildEnvironment, timeoutMilliseconds: 90 * 60_000, outputBound: 16 * 1024 * 1024,
        onOutput: (chunk) => { appendFileSync(logPath, chunk); process.stdout.write(chunk); },
      });
      const suffix = target === "darwin-aarch64" ? ".app.tar.gz" : "-setup.exe";
      const packages = await findPackage(path.join(source, "src-tauri/target/release/bundle"), suffix);
      if (packages.length !== 1) throw new Error("测试构建 updater 包不唯一");
      const destination = path.join(output, `${stage}${suffix}`);
      await cp(packages[0], destination);
      await cp(`${packages[0]}.sig`, `${destination}.sig`);
      const bytes = await readFile(destination);
      const signature = await readFile(`${destination}.sig`, "utf8");
      verifyTauriUpdaterSignature(bytes, signature, publicKey);
      receipt.builds[stage] = { sourceSha, sourceVersion, version, overlay, overlaySha256: sha256(overlay), packageSha256: sha256(bytes), signatureSha256: sha256(signature), package: path.basename(destination), signature: `${path.basename(destination)}.sig` };
      // 专为本次打包创建的导出树不是开发 worktree；保留源码/补丁/日志，回收已导出的构建产物。
      await rm(path.join(source, "src-tauri/target"), { recursive: true, force: true });
      await rm(path.join(source, "node_modules"), { recursive: true, force: true });
    }
    await writeFile(path.join(output, "test-builds.json"), `${JSON.stringify(receipt, null, 2)}\n`);
    return receipt;
  } finally {
    await rm(privateKeyPath, { force: true });
    delete buildEnvironment.TAURI_SIGNING_PRIVATE_KEY;
  }
}

/** 命令行只接受两个源码提交、版本与独立输出目录。 */
async function main() {
  const [baselineSha, baselineVersion, candidateSha, candidateVersion, output] = process.argv.slice(2);
  if (!output) throw new Error("需要 baseline SHA/version、candidate SHA/version 和输出目录");
  await buildUpdateTestPackages({ baselineSha, baselineVersion, candidateSha, candidateVersion, output });
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main().catch((error) => { process.stderr.write(`${error.message}\n`); process.exitCode = 1; });
}
