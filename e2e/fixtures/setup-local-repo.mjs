// 构造本地 Git fixture 仓库,供真实 Tauri E2E(local-comparison)使用。
// 用系统 Git CLI 搭建真实仓库夹具，应用通过同一 Core Git 语义读取它。
// 用法: node setup-local-repo.mjs <目标目录>
import { rm, mkdir, writeFile } from "node:fs/promises";
import { execFileSync } from "node:child_process";
import { resolve } from "node:path";

const target = resolve(process.argv[2] ?? "./narrant-fixture");

await rm(target, { recursive: true, force: true });
await mkdir(target, { recursive: true });

function git(...args) {
  execFileSync("git", args, { cwd: target, stdio: "inherit" });
}

git("init", "-q", "--initial-branch=main");
git("config", "user.email", "test@narrant.invalid");
git("config", "user.name", "Narrant test");

// 共同祖先:welcome.txt 为旧内容。
await writeFile(resolve(target, "welcome.txt"), "你好,旧内容\n", "utf8");
git("add", "welcome.txt");
git("commit", "-q", "-m", "base");
git("branch", "base-ref");
const baseSha = execFileSync("git", ["rev-parse", "HEAD"], { cwd: target, encoding: "utf8" }).trim();

// head 分支:welcome.txt 改为新内容。
git("checkout", "-q", "-b", "head-ref");
await writeFile(resolve(target, "welcome.txt"), "你好,新内容\n", "utf8");
git("commit", "-q", "-am", "head");
const headSha = execFileSync("git", ["rev-parse", "HEAD"], { cwd: target, encoding: "utf8" }).trim();
git("update-ref", "refs/pull/42/head", headSha);
git("remote", "add", "origin", "https://github.com/octo-org/octo-repo.git");
git("config", `url.file://${target}/.insteadOf`, "https://github.com/octo-org/octo-repo.git");

console.log(JSON.stringify({ target, baseSha, headSha }));
