import { execFileSync } from "node:child_process";
import { readFileSync, realpathSync } from "node:fs";
import { basename } from "node:path";

/** Git trace2 中与一次子进程启动相关的可验证字段。 */
export type GitInvocation = {
  argv: string[];
  cwd: string;
};

/** 按操作系统文件身份比较路径，兼容 Windows 扩展路径前缀与分隔符。 */
export function sameFilesystemPath(left: string, right: string): boolean {
  const normalize = (path: string) => {
    const canonical = realpathSync.native(path).replace(/^\\\\\?\\/, "").replaceAll("\\", "/");
    return process.platform === "win32" ? canonical.toLocaleLowerCase("en-US") : canonical;
  };
  try {
    return normalize(left) === normalize(right);
  } catch {
    return false;
  }
}

/** 读取 fixture 仓库当前注册的 linked worktree 绝对路径。 */
export function linkedWorktrees(repositoryPath: string): string[] {
  const environment = { ...process.env };
  delete environment.GIT_TRACE2_EVENT;
  return execFileSync("git", ["worktree", "list", "--porcelain", "-z"], {
    cwd: repositoryPath,
    encoding: "utf8",
    env: environment,
  }).split("\0")
    .filter((field) => field.startsWith("worktree "))
    .map((field) => field.slice("worktree ".length));
}

/** 等待 trace 文件以完整换行结束并保持静默，避免接受仍在写入的半行。 */
async function stableTraceBytes(tracePath: string, offset: number, timeout = 5000) {
  const deadline = Date.now() + timeout;
  let previousSize = -1;
  let stableSince = Date.now();
  while (Date.now() < deadline) {
    // 文件缺失、读取失败或 offset 回退都直接抛错，观测证据不能降级为空结果。
    const trace = readFileSync(tracePath);
    if (trace.length < offset) throw new Error(`Git trace 长度 ${trace.length} 小于观测 offset ${offset}`);
    if (trace.length !== previousSize) {
      previousSize = trace.length;
      stableSince = Date.now();
    }
    const complete = trace.length === 0 || trace.at(-1) === 0x0a;
    if (complete && Date.now() - stableSince >= 200) return trace.subarray(offset);
    await new Promise((resolve) => setTimeout(resolve, 25));
  }
  throw new Error(`Git trace 在 ${timeout}ms 内没有形成稳定的完整行：${tracePath}`);
}

/** 严格解析 trace2；start 的工作目录来自同一 session 随后的 def_repo 事件。 */
function parseInvocations(trace: Buffer): GitInvocation[] {
  const text = trace.toString("utf8");
  if (!text) return [];
  const starts: Array<{ sid: string; argv: string[]; line: number }> = [];
  const worktrees = new Map<string, string>();
  text.trimEnd().split("\n").forEach((line, index) => {
    let event: { event?: unknown; sid?: unknown; argv?: unknown; worktree?: unknown };
    try {
      event = JSON.parse(line) as typeof event;
    } catch (reason) {
      throw new Error(`Git trace 第 ${index + 1} 行不是完整 JSON`, { cause: reason });
    }
    if (event.event === "start") {
      if (typeof event.sid !== "string" || !Array.isArray(event.argv) || !event.argv.every((part) => typeof part === "string")) {
        throw new Error(`Git trace 第 ${index + 1} 行缺少 start sid/argv`);
      }
      starts.push({ sid: event.sid, argv: event.argv as string[], line: index + 1 });
    } else if (event.event === "def_repo") {
      if (typeof event.sid !== "string" || typeof event.worktree !== "string") {
        throw new Error(`Git trace 第 ${index + 1} 行缺少 def_repo sid/worktree`);
      }
      worktrees.set(event.sid, event.worktree);
    }
  });
  return starts.map((start) => {
    const changeDirectory = start.argv.indexOf("-C");
    const helperName = basename(start.argv[0]).replace(/\.exe$/i, "");
    const repositoryHelper = ["git-upload-pack", "git-receive-pack", "git-upload-archive"]
      .includes(helperName) ? start.argv.at(-1) : undefined;
    const cwd = worktrees.get(start.sid)
      ?? (changeDirectory >= 0 ? start.argv[changeDirectory + 1] : undefined)
      // Git transport helper 不进入 repository setup；其最后一个参数就是被服务的 Repository。
      ?? repositoryHelper;
    if (!cwd) throw new Error(`Git trace 第 ${start.line} 行的 start 缺少同 session def_repo：${JSON.stringify(start.argv)}`);
    return { argv: start.argv, cwd };
  });
}

/** 等待现有 trace 稳定并返回下一段观测的精确字节 offset。 */
export async function stableGitTraceOffset(tracePath: string): Promise<number> {
  const trace = await stableTraceBytes(tracePath, 0);
  parseInvocations(trace);
  return trace.length;
}

/** 等待指定 offset 之后的 trace 稳定，再返回所有 Git start invocation。 */
export async function gitInvocationsSinceStable(tracePath: string, offset: number): Promise<GitInvocation[]> {
  return parseInvocations(await stableTraceBytes(tracePath, offset));
}

/** 过滤所有会重建、迁移、释放或重置 Workspace 内容的 Git invocation。 */
export function workspaceMutationInvocations(invocations: GitInvocation[]): GitInvocation[] {
  return invocations.filter(({ argv }) => {
    const worktreeIndex = argv.indexOf("worktree");
    if (worktreeIndex >= 0 && ["add", "move", "remove", "prune", "lock", "unlock"].includes(argv[worktreeIndex + 1])) return true;
    return argv.some((argument) => ["reset", "checkout", "switch"].includes(argument));
  });
}

/** 过滤 Workspace 生命周期或针对已保留 Workspace 的任何读取/检查。 */
export function workspaceOperationInvocations(invocations: GitInvocation[], workspacePaths: string[]): GitInvocation[] {
  return invocations.filter((invocation) => {
    const { argv } = invocation;
    // worktree list 也属于 Workspace inspection；即使 argv 没有具体路径也必须禁止。
    if (argv.includes("worktree")) return true;
    if (argv.some((argument) => ["reset", "checkout", "switch"].includes(argument))) return true;
    return workspacePaths.some((workspacePath) => invocationTouchesWorkspace(invocation, workspacePath));
  });
}

/** 判断一次 Git invocation 是否在指定 Workspace 内执行或把该路径作为参数。 */
export function invocationTouchesWorkspace(invocation: GitInvocation, workspacePath: string): boolean {
  return invocation.cwd === workspacePath
    || invocation.cwd.startsWith(`${workspacePath}/`)
    || invocation.argv.some((argument) => argument === workspacePath || argument.startsWith(`${workspacePath}/`));
}
