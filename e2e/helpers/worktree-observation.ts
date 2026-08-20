import { readFileSync, statSync } from "node:fs";
import { execFileSync } from "node:child_process";

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

/** 返回应用启动后 Git trace 文件的当前字节偏移。 */
export function gitTraceOffset(tracePath: string): number {
  try {
    return statSync(tracePath).size;
  } catch {
    return 0;
  }
}

/** 读取指定偏移之后由 Git 启动的机器格式命令。 */
export function gitCommandsSince(tracePath: string, offset: number): string[][] {
  const trace = (() => {
    try {
      return readFileSync(tracePath, "utf8").slice(offset);
    } catch {
      return "";
    }
  })();
  if (!trace) return [];
  return trace.split("\n").flatMap((line) => {
    if (!line) return [];
    try {
      const event = JSON.parse(line) as { event?: string; argv?: unknown };
      return event.event === "start" && Array.isArray(event.argv) && event.argv.every((part) => typeof part === "string")
        ? [event.argv as string[]]
        : [];
    } catch {
      // Git 可能正在追加一行；下一次读取会从同一个稳定偏移重新解析。
      return [];
    }
  });
}

/** 过滤会改变 Workspace 的 Git 命令，避免把只读 worktree list 观察误判成 materialize。 */
export function workspaceCommands(commands: string[][]): string[][] {
  return commands.filter((argv) => {
    const worktreeIndex = argv.indexOf("worktree");
    if (worktreeIndex < 0) return false;
    return ["add", "move", "remove", "prune", "lock", "unlock"].includes(argv[worktreeIndex + 1]);
  });
}
