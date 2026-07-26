import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { DiffEditor } from "@monaco-editor/react";
import { openUrl } from "@tauri-apps/plugin-opener";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { useSystemTheme } from "@/lib/use-system-theme";

type ChangeStatus = "added" | "modified" | "deleted" | "renamed";
type FileChangeSummary = {
  path: string;
  oldPath: string | null;
  status: ChangeStatus;
};
type FileChangeContent = FileChangeSummary & {
  contentKind: "text" | "binary" | "submodule" | "symlink";
  beforeSize: number | null;
  afterSize: number | null;
  before: string | null;
  after: string | null;
};
type ReviewSnapshotBase = {
  sessionId: string;
  baseRef: string;
  baseRevision: string;
  comparisonBase: string;
  headRef: string;
  headRevision: string;
  fileChanges: FileChangeSummary[];
};
type LocalReviewSnapshot = ReviewSnapshotBase & { source: "local" };
type GitHubReviewSnapshot = ReviewSnapshotBase & {
  source: "github";
  owner: string;
  repository: string;
  title: string;
  number: number;
  author: string;
  state: "open" | "closed" | "merged";
  draft: boolean;
  description: string;
  changedFiles: number;
  additions: number;
  deletions: number;
  githubUrl: string;
};
type ReviewSnapshot = LocalReviewSnapshot | GitHubReviewSnapshot;
type GitHubAuthStatus = {
  configured: boolean;
  signedIn: boolean;
  appInstallUrl: string | null;
};
type DeviceAuthorization = {
  authorizationId: string;
  userCode: string;
  verificationUri: string;
  expiresIn: number;
  interval: number;
};

type DirectoryNode = {
  name: string;
  directories: DirectoryNode[];
  files: FileChangeSummary[];
};

function buildDirectoryTree(fileChanges: FileChangeSummary[]): DirectoryNode {
  const root: DirectoryNode = { name: "", directories: [], files: [] };
  for (const change of fileChanges) {
    const parts = change.path.split("/");
    let directory = root;
    for (const name of parts.slice(0, -1)) {
      let child = directory.directories.find((candidate) => candidate.name === name);
      if (!child) {
        child = { name, directories: [], files: [] };
        directory.directories.push(child);
      }
      directory = child;
    }
    directory.files.push(change);
  }
  const sort = (directory: DirectoryNode) => {
    directory.directories.sort((left, right) => left.name.localeCompare(right.name));
    directory.files.sort((left, right) => left.path.localeCompare(right.path));
    directory.directories.forEach(sort);
  };
  sort(root);
  return root;
}

function ErrorMessage({ message }: { message: string }) {
  const match = message.match(/(https:\/\/github\.com\/[^\s]+)/);
  const text = match ? message.replace(match[1], "") : message;
  return <p role="alert">
    {text}
    {match && <> <a href={match[1]} onClick={(event) => { event.preventDefault(); void openUrl(match[1]); }}>在 GitHub 查看</a></>}
  </p>;
}

export function App() {
  const editorTheme = useSystemTheme();
  const [repositoryPath, setRepositoryPath] = useState("");
  const [pullRequestUrl, setPullRequestUrl] = useState("");
  const [refs, setRefs] = useState<string[]>([]);
  const [baseRef, setBaseRef] = useState("");
  const [headRef, setHeadRef] = useState("");
  const [snapshot, setSnapshot] = useState<ReviewSnapshot>();
  const [fileChange, setFileChange] = useState<FileChangeContent>();
  const [error, setError] = useState("");
  const [loading, setLoading] = useState(false);
  const [authStatus, setAuthStatus] = useState<GitHubAuthStatus>();
  const [deviceAuthorization, setDeviceAuthorization] = useState<DeviceAuthorization>();
  const [authorizing, setAuthorizing] = useState(false);
  const [copiedValue, setCopiedValue] = useState<"device-code" | "authorization-url">();
  const activeSessionId = useRef<string>();
  const activeGitOperationId = useRef<string>();
  const nextGitOperationId = useRef(0);

  /** 执行可取消的本地 Core Git 命令，并在新操作开始时取消旧操作。 */
  async function invokeLocalGit<T>(command: string, arguments_: Record<string, unknown>) {
    const previousOperationId = activeGitOperationId.current;
    if (previousOperationId) {
      void Promise.resolve(invoke("cancel_git_operation", { operationId: previousOperationId }))
        .catch(() => undefined);
    }
    const operationId = `local-git-${nextGitOperationId.current++}`;
    activeGitOperationId.current = operationId;
    try {
      return await invoke<T>(command, { ...arguments_, operationId });
    } finally {
      if (activeGitOperationId.current === operationId) activeGitOperationId.current = undefined;
    }
  }

  function replaceSnapshot(nextSnapshot?: ReviewSnapshot) {
    const previousSessionId = activeSessionId.current;
    activeSessionId.current = nextSnapshot?.sessionId;
    setSnapshot(nextSnapshot);
    if (previousSessionId && previousSessionId !== nextSnapshot?.sessionId) {
      void Promise.resolve(invoke("close_review_session", { sessionId: previousSessionId }))
        .catch(() => undefined);
    }
  }

  useEffect(() => () => {
    if (activeGitOperationId.current) {
      void Promise.resolve(invoke("cancel_git_operation", { operationId: activeGitOperationId.current }))
        .catch(() => undefined);
      activeGitOperationId.current = undefined;
    }
    if (activeSessionId.current) {
      void Promise.resolve(invoke("close_review_session", { sessionId: activeSessionId.current }))
        .catch(() => undefined);
      activeSessionId.current = undefined;
    }
  }, []);

  async function openPullRequest() {
    setError("");
    setLoading(true);
    try {
      const nextSnapshot = await invoke<ReviewSnapshot>("open_github_pull_request", { url: pullRequestUrl });
      replaceSnapshot(nextSnapshot);
      setFileChange(undefined);
    } catch (reason) {
      replaceSnapshot(undefined);
      setFileChange(undefined);
      setError(String(reason));
    } finally {
      setLoading(false);
    }
  }

  /** 启动连接向导，并在已配置时将用户带到 GitHub App 安装页。 */
  async function connectGitHub() {
    setError("");
    try {
      const status = await invoke<GitHubAuthStatus>("github_auth_status");
      setAuthStatus(status);
      if (status.configured && !status.signedIn && status.appInstallUrl) {
        await openUrl(status.appInstallUrl);
      }
    } catch (reason) {
      setError(String(reason));
    }
  }

  async function startGitHubAuthorization() {
    setError("");
    setCopiedValue(undefined);
    setAuthorizing(true);
    try {
      const authorization = await invoke<DeviceAuthorization>("start_github_authorization");
      setDeviceAuthorization(authorization);
      await openUrl(authorization.verificationUri);
    } catch (reason) {
      setError(String(reason));
    } finally {
      setAuthorizing(false);
    }
  }

  /** 将用户主动选择的授权信息复制到系统剪贴板，并反馈复制结果。 */
  async function copyAuthorizationValue(value: string, copied: "device-code" | "authorization-url") {
    setError("");
    try {
      await navigator.clipboard.writeText(value);
      setCopiedValue(copied);
    } catch (reason) {
      setError(`无法复制到剪贴板：${String(reason)}`);
    }
  }

  async function completeGitHubAuthorization() {
    if (!deviceAuthorization) return;
    setError("");
    setAuthorizing(true);
    try {
      await invoke("complete_github_authorization", {
        authorizationId: deviceAuthorization.authorizationId,
      });
      setAuthStatus((status) => status ? { ...status, signedIn: true } : status);
      setDeviceAuthorization(undefined);
    } catch (reason) {
      setError(String(reason));
    } finally {
      setAuthorizing(false);
    }
  }

  async function signOutGitHub() {
    setError("");
    try {
      await invoke("sign_out_github");
      if (snapshot?.source === "github") {
        replaceSnapshot(undefined);
        setFileChange(undefined);
      }
      setAuthStatus((status) => status ? { ...status, signedIn: false } : status);
      setDeviceAuthorization(undefined);
    } catch (reason) {
      setError(String(reason));
    }
  }

  async function loadRefs() {
    setError("");
    try {
      const loadedRefs = await invokeLocalGit<string[]>("list_local_refs", { repositoryPath });
      setRefs(loadedRefs);
      replaceSnapshot(undefined);
      setFileChange(undefined);
      setBaseRef(loadedRefs[0] ?? "");
      setHeadRef(loadedRefs[1] ?? loadedRefs[0] ?? "");
    } catch (reason) {
      setRefs([]);
      replaceSnapshot(undefined);
      setFileChange(undefined);
      setError(String(reason));
    }
  }

  async function openComparison() {
    setError("");
    try {
      replaceSnapshot(await invokeLocalGit<ReviewSnapshot>("compare_local_refs", { repositoryPath, baseRef, headRef }));
      setFileChange(undefined);
    }
    catch (reason) { replaceSnapshot(undefined); setFileChange(undefined); setError(String(reason)); }
  }

  async function openFileChange(path: string) {
    if (!snapshot) return;
    setError("");
    try {
      const arguments_ = { sessionId: snapshot.sessionId, path };
      setFileChange(snapshot.source === "local"
        ? await invokeLocalGit<FileChangeContent>("read_file_change", arguments_)
        : await invoke<FileChangeContent>("read_file_change", arguments_));
    } catch (reason) {
      setFileChange(undefined);
      setError(String(reason));
    }
  }

  function renderDirectory(directory: DirectoryNode) {
    return <>
      {directory.directories.map((child) => <li key={child.name} role="treeitem" aria-label={child.name} aria-expanded="true">
        <span>{child.name}</span>
        <ul role="group">{renderDirectory(child)}</ul>
      </li>)}
      {directory.files.map((change) => <li key={change.path} role="none">
        <Button variant="ghost" type="button" role="treeitem" onClick={() => openFileChange(change.path)}>
          <span>{change.oldPath ? `${change.oldPath} → ${change.path}` : change.path.split("/").at(-1)}</span> <small>{change.status}</small>
        </Button>
      </li>)}
    </>;
  }

  return <main>
    <header><h1>Narrant</h1><p>只读变更审核</p></header>
    <section aria-label="打开 GitHub Pull Request" className="flex flex-wrap items-end gap-3">
      <label>GitHub Pull Request URL<Input aria-label="GitHub Pull Request URL" type="url" placeholder="https://github.com/owner/repo/pull/123" value={pullRequestUrl} onChange={(event) => setPullRequestUrl(event.target.value)} /></label>
      <Button disabled={!pullRequestUrl || loading} onClick={openPullRequest}>{loading ? "正在加载…" : authStatus?.signedIn ? "打开" : "匿名打开"}</Button>
      {!authStatus && <Button variant="outline" onClick={connectGitHub}>连接 GitHub</Button>}
      {authStatus?.signedIn && <Button variant="outline" onClick={signOutGitHub}>退出 GitHub</Button>}
    </section>
    {authStatus && !authStatus.signedIn && <section aria-label="GitHub 授权">
      {!authStatus.configured
        ? <p>此构建未配置 GitHub App，仍可匿名打开公开 Pull Request。</p>
        : <>
          <p>第 1 步：在 GitHub 中安装 Narrant，并为目标仓库授予只读访问；组织仓库可能需要管理员审批。</p>
          {authStatus.appInstallUrl && <Button variant="outline" onClick={() => openUrl(authStatus.appInstallUrl!)}>重新选择仓库或申请组织审批</Button>}
          <p>第 2 步：安装完成后返回此处，继续授权当前 GitHub 账号。</p>
          <Button disabled={authorizing} onClick={startGitHubAuthorization}>我已安装，继续</Button>
          {deviceAuthorization && <div>
            <p>设备码：<strong>{deviceAuthorization.userCode}</strong></p>
            <Button variant="outline" type="button" onClick={() => void copyAuthorizationValue(deviceAuthorization.userCode, "device-code")}>
              {copiedValue === "device-code" ? "设备码已复制" : "复制设备码"}
            </Button>
            <p>授权 URL：<a href={deviceAuthorization.verificationUri} onClick={(event) => { event.preventDefault(); void openUrl(deviceAuthorization.verificationUri); }}>{deviceAuthorization.verificationUri}</a></p>
            <div className="flex flex-wrap gap-3">
              <Button variant="outline" type="button" onClick={() => void copyAuthorizationValue(deviceAuthorization.verificationUri, "authorization-url")}>
                {copiedValue === "authorization-url" ? "链接已复制" : "复制链接"}
              </Button>
              <Button variant="outline" type="button" onClick={() => void openUrl(deviceAuthorization.verificationUri)}>打开 GitHub</Button>
            </div>
            <p>仅使用 Narrant 当前生成的设备码，并在 GitHub 页面核对应用名称。不要输入他人发送的验证码。</p>
            <Button disabled={authorizing} onClick={completeGitHubAuthorization}>我已在 GitHub 完成授权</Button>
          </div>}
        </>}
    </section>}
    <p>本地分支比较</p>
    <section aria-label="选择本地仓库" className="flex flex-wrap items-end gap-3">
      <label>本地仓库路径<Input aria-label="本地仓库路径" value={repositoryPath} onChange={(event) => setRepositoryPath(event.target.value)} /></label>
      <Button variant="outline" onClick={loadRefs}>读取分支</Button>
      <label>Base Ref<Select value={baseRef} onValueChange={(value) => setBaseRef(value ?? "")}><SelectTrigger aria-label="Base Ref"><SelectValue placeholder="选择 Base Ref" /></SelectTrigger><SelectContent>{refs.map((ref) => <SelectItem key={ref} value={ref}>{ref}</SelectItem>)}</SelectContent></Select></label>
      <label>Head Ref<Select value={headRef} onValueChange={(value) => setHeadRef(value ?? "")}><SelectTrigger aria-label="Head Ref"><SelectValue placeholder="选择 Head Ref" /></SelectTrigger><SelectContent>{refs.map((ref) => <SelectItem key={ref} value={ref}>{ref}</SelectItem>)}</SelectContent></Select></label>
      <Button disabled={!baseRef || !headRef} onClick={openComparison}>打开 Review Snapshot</Button>
    </section>
    {error && <ErrorMessage message={error} />}
    {snapshot?.source === "github" && <article aria-label="Pull Request 概览">
      <h2>{snapshot.title}</h2>
      <p>#{snapshot.number} · {snapshot.author} · {snapshot.draft ? "草稿 · " : ""}{{ open: "开放", closed: "已关闭", merged: "已合并" }[snapshot.state ?? "open"]}</p>
      <p>{snapshot.baseRef} @ {snapshot.baseRevision} → {snapshot.headRef} @ {snapshot.headRevision}</p>
      <p>{snapshot.changedFiles} 个文件 · +{snapshot.additions} · −{snapshot.deletions}</p>
      <div aria-label="Pull Request 描述">
        <ReactMarkdown
          remarkPlugins={[remarkGfm]}
          skipHtml
          components={{
            img: ({ alt }) => <span>{alt ? `[图片：${alt}，已阻止远程加载]` : "[已阻止远程图片]"}</span>,
            a: ({ href, children }) => <a href={href} onClick={(event) => {
              event.preventDefault();
              if (href?.startsWith("https://") || href?.startsWith("http://")) void openUrl(href);
            }}>{children}</a>,
          }}
        >{snapshot.description}</ReactMarkdown>
      </div>
    </article>}
    {snapshot && <div className="grid grid-cols-[minmax(16rem,24rem)_1fr] gap-4">
      <nav aria-label="文件树">
        <ul role="tree" aria-label="文件树">
          {renderDirectory(buildDirectoryTree(snapshot.fileChanges))}
        </ul>
      </nav>
      {snapshot.fileChanges.length === 0 && <p>没有 File Change</p>}
      {fileChange && <section aria-label="File Change" data-before={import.meta.env.VITE_WDIO ? fileChange.before ?? "" : undefined} data-after={import.meta.env.VITE_WDIO ? fileChange.after ?? "" : undefined}>
        <h2>{fileChange.path} <small>{fileChange.status}</small></h2>
        {fileChange.contentKind === "binary"
          ? <p>二进制文件（变更前：{fileChange.beforeSize === null ? "无" : `${fileChange.beforeSize} 字节`}；变更后：{fileChange.afterSize === null ? "无" : `${fileChange.afterSize} 字节`}）</p>
          : <>
            {fileChange.contentKind === "submodule" && <p>Submodule commit</p>}
            {fileChange.contentKind === "symlink" && <p>Symlink 目标</p>}
            <DiffEditor theme={editorTheme} height="500px" language="plaintext" original={fileChange.before ?? ""} modified={fileChange.after ?? ""} options={{ readOnly: true, renderSideBySide: true, ignoreTrimWhitespace: false }} />
          </>}
      </section>}
    </div>}
  </main>;
}
