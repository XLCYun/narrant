import { useTranslation } from "react-i18next";
import { isReferenceVisible, objectDetails } from "@/structure-review/graph-presentation";
import type { CodeNode, CodeReference, StructureComparison } from "@/structure-review/types";

/** 常驻详情面板在对象信息和文件覆盖范围之间切换。 */
export type StructureSelection = { kind: "object"; id: string } | { kind: "coverage" };

/** 统一呈现选中对象的两侧内容及可见关联，宽度和画布状态由外层分栏负责。 */
export function StructureDetails({ comparison, selection, showImports }: {
  comparison: StructureComparison; selection?: StructureSelection; showImports: boolean;
}) {
  const { t } = useTranslation("structure");
  const details = selection?.kind === "object" ? objectDetails(comparison, selection.id) : undefined;
  const selected = details?.after ?? details?.before;
  const referenceChanges = details?.difference?.references.filter((change) => isReferenceVisible(change.value, { showImports })) ?? [];
  const selectedReferences = selected?.references.filter((reference) => isReferenceVisible(reference, { showImports })) ?? [];
  return <aside className="structure-details flex h-full min-w-0 flex-col bg-muted/20" aria-label={t("detailsPanel")}>
    <header className="shrink-0 border-b bg-background/95 px-3 py-3">
      <h3 className="text-sm font-semibold">{t(selection?.kind === "coverage" ? "coverageTitle" : "detailsTitle")}</h3>
      <p className="mt-1 text-xs text-muted-foreground">{t(selection?.kind === "coverage" ? "coverageDescription" : "detailsDescription")}</p>
    </header>
    <div key={JSON.stringify(selection)} className="min-h-0 flex-1 overflow-auto p-3" data-structure-detail-content>
      {!selection && <p className="py-5 text-sm text-muted-foreground">{t("selectObject")}</p>}
      {selection?.kind === "coverage" ? <ul className="space-y-3">{comparison.coverage.map((file) => <li key={file.path} className="rounded-md border p-3 text-xs">
        <p className="break-all font-mono">{file.oldPath && file.oldPath !== file.path ? `${file.oldPath} → ${file.path}` : file.path}</p>
        <p className="mt-2 text-muted-foreground">{t("before")} · {t(`coverageKind.${file.before}`)}<br />{t("after")} · {t(`coverageKind.${file.after}`)}</p>
      </li>)}</ul> : selected && details && <>
        <h3 className="break-words font-mono text-sm font-medium">{selected.name ?? selected.signature ?? selected.nativeKind}</h3>
        <p className="mt-1 text-xs text-muted-foreground">{selected.language} · {selected.nativeKind} · {t(`status.${details.difference?.content ?? (details.difference ? "relations" : "unchanged")}`)}</p>
        <div className="mt-5 grid gap-4">
          <DeclarationSide title={t("before")} node={details.before} />
          <DeclarationSide title={t("after")} node={details.after} />
        </div>
        <h4 className="mb-2 mt-6 text-sm font-medium">{t("relationshipChanges")}</h4>
        {(details.difference?.contains.length ?? 0) + referenceChanges.length === 0
          ? <p className="text-xs text-muted-foreground">{t("noRelationshipChanges")}</p>
          : <ul className="space-y-2 text-xs">{details.difference?.contains.map((change) => <li key={`${change.kind}-${change.value}`} className="break-words"><span className="font-medium">{t(`status.${change.kind}`)}</span> · contains → {objectName(comparison, change.value)}</li>)}
            {referenceChanges.map((change, index) => <li key={index} className="break-words"><span className="font-medium">{t(`status.${change.kind}`)}</span> · {referenceLabel(comparison, change.value)}</li>)}
          </ul>}
        <h4 className="mb-2 mt-6 text-sm font-medium">{t("referencesTitle")}</h4>
        {selectedReferences.length ? <ul className="space-y-2 break-words text-xs text-muted-foreground">{selectedReferences.map((reference, index) => <li key={index}>{referenceLabel(comparison, reference)}</li>)}</ul> : <p className="text-xs text-muted-foreground">{t("noVisibleReferences")}</p>}
      </>}
    </div>
  </aside>;
}

/** 分侧显示源码；不存在的节点与没有实现内容的声明使用不同提示。 */
function DeclarationSide({ title, node }: { title: string; node?: CodeNode }) {
  const { t } = useTranslation("structure");
  return <section className="min-w-0"><h4 className="mb-2 text-xs font-medium">{title}</h4>
    {!node ? <p className="text-xs text-muted-foreground">{t("absent")}</p> : <>
      <p className="mb-2 break-words text-xs text-muted-foreground">{node.name ?? node.nativeKind} · {node.language} · {node.nativeKind}</p>
      <pre className="overflow-auto rounded-md border bg-muted/30 p-3 text-[11px] leading-relaxed">{node.signature ?? t("noSignature")}</pre>
      <h5 className="mb-2 mt-3 text-xs text-muted-foreground">{t("ownCode")}</h5>
      <pre className="max-h-80 overflow-auto rounded-md border bg-muted/30 p-3 text-[11px] leading-relaxed">{node.code ?? t("noCode")}</pre>
      {Object.keys(node.extensions).length > 0 && <><h5 className="mb-2 mt-3 text-xs text-muted-foreground">{t("languageDetails")}</h5><pre className="max-h-60 overflow-auto rounded-md border bg-muted/30 p-3 text-[11px]">{JSON.stringify(node.extensions, null, 2)}</pre></>}
    </>}
  </section>;
}

/** 按节点身份读取显示名，不从 ID 编码制造源文件位置。 */
function objectName(comparison: StructureComparison, id: string): string {
  const node = comparison.after.nodes.find((item) => item.id === id) ?? comparison.before.nodes.find((item) => item.id === id);
  return node?.name ?? node?.signature ?? node?.nativeKind ?? id;
}

/** 详情只格式化已经通过展示过滤的图内引用。 */
function referenceLabel(comparison: StructureComparison, reference: CodeReference): string {
  const target = reference.target;
  return target.kind === "node" ? `${reference.semantic} → ${objectName(comparison, target.id)}` : "";
}
