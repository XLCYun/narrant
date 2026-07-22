import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { DiffEditor } from "@monaco-editor/react";
import { openUrl } from "@tauri-apps/plugin-opener";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { useSystemTheme } from "@/lib/use-system-theme";

type FileChange = {
  path: string;
  oldPath: string | null;
  status: "added" | "modified" | "deleted" | "renamed";
  contentKind: "text" | "binary" | "submodule" | "symlink";
  beforeSize: number | null;
  afterSize: number | null;
  before: string | null;
  after: string | null;
};
type ReviewSnapshot = {
  snapshotId: string;
  owner?: string;
  repository?: string;
  title?: string;
  number?: number;
  author?: string;
  state?: "open" | "closed" | "merged";
  draft?: boolean;
  baseRef?: string;
  baseRevision?: string;
  comparisonBase: string;
  headRef?: string;
  headRevision: string;
  description?: string;
  changedFiles?: number;
  additions?: number;
  deletions?: number;
  githubUrl?: string;
  fileChanges: FileChange[];
};

type DirectoryNode = {
  name: string;
  directories: DirectoryNode[];
  files: FileChange[];
};

function buildDirectoryTree(fileChanges: FileChange[]): DirectoryNode {
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
  const [fileChange, setFileChange] = useState<FileChange>();
  const [error, setError] = useState("");
  const [loading, setLoading] = useState(false);

  async function openPullRequest() {
    setError("");
    setLoading(true);
    try {
      const nextSnapshot = await invoke<ReviewSnapshot>("open_github_pull_request", { url: pullRequestUrl });
      setSnapshot(nextSnapshot);
      setFileChange(undefined);
    } catch (reason) {
      setSnapshot(undefined);
      setFileChange(undefined);
      setError(String(reason));
    } finally {
      setLoading(false);
    }
  }

  async function loadRefs() {
    setError("");
    try {
      const loadedRefs = await invoke<string[]>("list_local_refs", { repositoryPath });
      setRefs(loadedRefs);
      setSnapshot(undefined);
      setFileChange(undefined);
      setBaseRef(loadedRefs[0] ?? "");
      setHeadRef(loadedRefs[1] ?? loadedRefs[0] ?? "");
    } catch (reason) {
      setRefs([]);
      setError(String(reason));
    }
  }

  async function openComparison() {
    setError("");
    try {
      setSnapshot(await invoke<ReviewSnapshot>("compare_local_refs", { repositoryPath, baseRef, headRef }));
      setFileChange(undefined);
    }
    catch (reason) { setSnapshot(undefined); setFileChange(undefined); setError(String(reason)); }
  }

  async function openFileChange(path: string) {
    if (!snapshot) return;
    setError("");
    try {
      setFileChange(await invoke<FileChange>("read_file_change", { snapshotId: snapshot.snapshotId, path }));
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
      <Button disabled={!pullRequestUrl || loading} onClick={openPullRequest}>{loading ? "正在加载…" : "匿名打开"}</Button>
    </section>
    <p>本地分支比较</p>
    <section aria-label="选择本地仓库" className="flex flex-wrap items-end gap-3">
      <label>本地仓库路径<Input aria-label="本地仓库路径" value={repositoryPath} onChange={(event) => setRepositoryPath(event.target.value)} /></label>
      <Button variant="outline" onClick={loadRefs}>读取分支</Button>
      <label>Base Ref<Select value={baseRef} onValueChange={(value) => setBaseRef(value ?? "")}><SelectTrigger aria-label="Base Ref"><SelectValue placeholder="选择 Base Ref" /></SelectTrigger><SelectContent>{refs.map((ref) => <SelectItem key={ref} value={ref}>{ref}</SelectItem>)}</SelectContent></Select></label>
      <label>Head Ref<Select value={headRef} onValueChange={(value) => setHeadRef(value ?? "")}><SelectTrigger aria-label="Head Ref"><SelectValue placeholder="选择 Head Ref" /></SelectTrigger><SelectContent>{refs.map((ref) => <SelectItem key={ref} value={ref}>{ref}</SelectItem>)}</SelectContent></Select></label>
      <Button disabled={!baseRef || !headRef} onClick={openComparison}>打开 Review Snapshot</Button>
    </section>
    {error && <ErrorMessage message={error} />}
    {snapshot?.title && <article aria-label="Pull Request 概览">
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
