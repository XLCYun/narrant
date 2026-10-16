import { Buffer } from "node:buffer";
import { createHash } from "node:crypto";
import { mkdir, mkdtemp, readFile, readdir, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import process from "node:process";

import { FIXED_STABLE_ENDPOINT } from "./public-draft-assets.mjs";
import { releaseAssetNames, verifyTauriUpdaterSignature } from "./release-assets.mjs";
import { runBoundedCommand, selectCommandEnvironment } from "./update-smoke-tools.mjs";

/** 计算正式文件的摘要，独立于测试包的签名和文件记录。 */
function sha256(bytes) { return createHash("sha256").update(bytes).digest("hex"); }

/** 查找解包后的正式主程序；只读取文件，不执行安装器或应用。 */
async function findExecutable(directory) {
  const result = [];
  for (const entry of await readdir(directory, { withFileTypes: true })) {
    const file = path.join(directory, entry.name);
    if (entry.isDirectory()) result.push(...await findExecutable(file));
    else if (entry.isFile() && entry.name === "narrant.exe") result.push(file);
  }
  return result;
}

/** 静态核验正式包签名、身份和编译期信任根，不启动正式 Narrant。 */
export async function verifyProductionUpdateArtifacts(options, environment = process.env) {
  const commandEnvironment = selectCommandEnvironment(environment);
  const command = (executable, args) => runBoundedCommand(executable, args, { environment: commandEnvironment, outputBound: 4 * 1024 * 1024 });
  const names = releaseAssetNames(options.version);
  const mac = options.target === "darwin-aarch64";
  const packageName = mac ? names.updater.darwin : names.updater.windows;
  const packagePath = path.join(options.candidateDirectory, packageName);
  const [bytes, signature, installer, manifestBytes] = await Promise.all([
    readFile(packagePath), readFile(`${packagePath}.sig`, "utf8"), readFile(options.candidateInstaller),
    readFile(path.join(options.candidateDirectory, "latest.json")),
  ]);
  verifyTauriUpdaterSignature(bytes, signature, environment.NARRANT_RELEASE_SIGNING_PUBLIC_KEY);
  const checks = { scope: "signatures-and-integrity-only", packageSha256: sha256(bytes), signatureSha256: sha256(signature), installerSha256: sha256(installer),
    manifestSha256: sha256(manifestBytes), baselineTrustRoot: "verified", codeSigning: mac ? "verified" : "not-required", notarization: mac ? "verified" : "not-required", productionAppLaunched: false };
  for (const [name, digest] of [[packageName, checks.packageSha256], [`${packageName}.sig`, checks.signatureSha256], [path.basename(options.candidateInstaller), checks.installerSha256], ["latest.json", checks.manifestSha256]]) {
    if (options.draftReceipt.assets.find((asset) => asset.name === name)?.sha256 !== digest) throw new Error("正式包与 Draft 回读记录不一致");
  }
  const previous = options.previous;
  const previousBytes = await readFile(previous.packagePath);
  const previousSignature = await readFile(previous.signaturePath, "utf8");
  verifyTauriUpdaterSignature(previousBytes, previousSignature, environment.NARRANT_PREVIOUS_RELEASE_SIGNING_PUBLIC_KEY);
  if (sha256(previousBytes) !== previous.packageSha256 || sha256(previousSignature) !== previous.signatureSha256) throw new Error("上一 Stable 包已改变");
  const temporary = await mkdtemp(path.join(tmpdir(), "narrant-production-package-check-"));
  try {
    for (const [stage, file, version, key] of [
      ["baseline", previous.packagePath, previous.version, environment.NARRANT_RELEASE_SIGNING_PUBLIC_KEY],
      ["candidate", packagePath, options.version, environment.NARRANT_UPDATER_PUBLIC_KEY],
    ]) {
      const directory = path.join(temporary, stage);
      await mkdir(directory);
      let executable;
      if (mac) {
        await command("tar", ["-xzf", file, "-C", directory]);
        const application = path.join(directory, "Narrant.app");
        const plist = path.join(application, "Contents/Info.plist");
        const identifier = (await command("/usr/libexec/PlistBuddy", ["-c", "Print :CFBundleIdentifier", plist])).trim();
        const installedVersion = (await command("/usr/libexec/PlistBuddy", ["-c", "Print :CFBundleShortVersionString", plist])).trim();
        const executableName = (await command("/usr/libexec/PlistBuddy", ["-c", "Print :CFBundleExecutable", plist])).trim();
        if (identifier !== "com.cyunlab.narrant" || installedVersion !== version || executableName !== "narrant") throw new Error("正式 app 身份或版本不一致");
        await command("codesign", ["--verify", "--deep", "--strict", application]);
        await command("xcrun", ["stapler", "validate", application]);
        await command("spctl", ["--assess", "--type", "execute", application]);
        executable = path.join(application, "Contents/MacOS", executableName);
      } else {
        await command("C:\\Program Files\\7-Zip\\7z.exe", ["x", "-y", `-o${directory}`, file]);
        const executables = await findExecutable(directory);
        if (executables.length !== 1) throw new Error("正式 NSIS 包主程序不唯一");
        executable = executables[0];
      }
      const binary = await readFile(executable);
      if (!binary.includes(Buffer.from(FIXED_STABLE_ENDPOINT)) || !binary.includes(Buffer.from(key))) throw new Error(`${stage} 正式程序缺少预期更新信任根`);
    }
    if (mac) {
      await command("codesign", ["--verify", "--strict", options.candidateInstaller]);
      await command("xcrun", ["stapler", "validate", options.candidateInstaller]);
      await command("spctl", ["--assess", "--type", "open", "--context", "context:primary-signature", options.candidateInstaller]);
    } else if (checks.packageSha256 !== checks.installerSha256) throw new Error("正式 Windows 安装包与 updater 包不一致");
    return checks;
  } finally { await rm(temporary, { recursive: true, force: true }); }
}
