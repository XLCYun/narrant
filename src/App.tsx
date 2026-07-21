import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { DiffEditor } from "@monaco-editor/react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";

type FileChange = { path: string; status: string; before: string | null; after: string | null };
type ReviewSnapshot = { fileChanges: FileChange[] };

export function App() {
  const [repositoryPath, setRepositoryPath] = useState("");
  const [refs, setRefs] = useState<string[]>([]);
  const [baseRef, setBaseRef] = useState("");
  const [headRef, setHeadRef] = useState("");
  const [snapshot, setSnapshot] = useState<ReviewSnapshot>();
  const [error, setError] = useState("");
  const fileChange = snapshot?.fileChanges[0];

  async function loadRefs() {
    setError("");
    try {
      const loadedRefs = await invoke<string[]>("list_local_refs", { repositoryPath });
      setRefs(loadedRefs);
      setBaseRef(loadedRefs[0] ?? "");
      setHeadRef(loadedRefs[1] ?? loadedRefs[0] ?? "");
    } catch (reason) {
      setRefs([]);
      setError(String(reason));
    }
  }

  async function openComparison() {
    setError("");
    try { setSnapshot(await invoke<ReviewSnapshot>("compare_local_refs", { repositoryPath, baseRef, headRef })); }
    catch (reason) { setSnapshot(undefined); setError(String(reason)); }
  }

  return <main>
    <header><h1>Narrant</h1><p>本地分支比较</p></header>
    <section aria-label="选择本地仓库" className="flex flex-wrap items-end gap-3">
      <label>本地仓库路径<Input aria-label="本地仓库路径" value={repositoryPath} onChange={(event) => setRepositoryPath(event.target.value)} /></label>
      <Button variant="outline" onClick={loadRefs}>读取分支</Button>
      <label>Base Ref<Select value={baseRef} onValueChange={(value) => setBaseRef(value ?? "")}><SelectTrigger aria-label="Base Ref"><SelectValue placeholder="选择 Base Ref" /></SelectTrigger><SelectContent>{refs.map((ref) => <SelectItem key={ref} value={ref}>{ref}</SelectItem>)}</SelectContent></Select></label>
      <label>Head Ref<Select value={headRef} onValueChange={(value) => setHeadRef(value ?? "")}><SelectTrigger aria-label="Head Ref"><SelectValue placeholder="选择 Head Ref" /></SelectTrigger><SelectContent>{refs.map((ref) => <SelectItem key={ref} value={ref}>{ref}</SelectItem>)}</SelectContent></Select></label>
      <Button disabled={!baseRef || !headRef} onClick={openComparison}>打开 File Change</Button>
    </section>
    {error && <p role="alert">无法读取本地分支比较：{error}</p>}
    {fileChange && <section aria-label="File Change" data-before={import.meta.env.VITE_WDIO ? fileChange.before ?? "" : undefined} data-after={import.meta.env.VITE_WDIO ? fileChange.after ?? "" : undefined}><h2>{fileChange.path} <small>{fileChange.status}</small></h2><DiffEditor height="500px" language="plaintext" original={fileChange.before ?? ""} modified={fileChange.after ?? ""} options={{ readOnly: true, renderSideBySide: true, ignoreTrimWhitespace: false }} /></section>}
  </main>;
}
