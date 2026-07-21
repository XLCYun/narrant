import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { DiffEditor } from "@monaco-editor/react";

type FileChange = { path: string; status: string; before: string | null; after: string | null };
type ReviewSnapshot = { fileChanges: FileChange[] };

export function App() {
  const [repositoryPath, setRepositoryPath] = useState("");
  const [baseRef, setBaseRef] = useState("base");
  const [headRef, setHeadRef] = useState("head");
  const [snapshot, setSnapshot] = useState<ReviewSnapshot>();
  const [error, setError] = useState("");
  const fileChange = snapshot?.fileChanges[0];

  async function openComparison() {
    setError("");
    try { setSnapshot(await invoke<ReviewSnapshot>("compare_local_refs", { repositoryPath, baseRef, headRef })); }
    catch (reason) { setSnapshot(undefined); setError(String(reason)); }
  }

  return <main>
    <header><h1>Narrant</h1><p>本地分支比较</p></header>
    <section aria-label="选择本地仓库">
      <label>测试仓库路径<input aria-label="测试仓库路径" value={repositoryPath} onChange={(event) => setRepositoryPath(event.target.value)} /></label>
      <label>Base Ref<input aria-label="Base Ref" value={baseRef} onChange={(event) => setBaseRef(event.target.value)} /></label>
      <label>Head Ref<input aria-label="Head Ref" value={headRef} onChange={(event) => setHeadRef(event.target.value)} /></label>
      <button onClick={openComparison}>打开 File Change</button>
    </section>
    {error && <p role="alert">无法读取本地分支比较：{error}</p>}
    {fileChange && <section aria-label="File Change"><h2>{fileChange.path} <small>{fileChange.status}</small></h2><DiffEditor height="500px" language="plaintext" original={fileChange.before ?? ""} modified={fileChange.after ?? ""} options={{ readOnly: true, renderSideBySide: true, ignoreTrimWhitespace: false }} /></section>}
  </main>;
}
