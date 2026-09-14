import { useTranslation } from "react-i18next";
import { isReferenceVisible, objectDetails } from "@/structure-review/graph-presentation";
import type { CodeNode, CodeReference, StructureComparison } from "@/structure-review/types";
import { StructureCodeDiff } from "./structure-code-diff";

/** 统一呈现选中对象的两侧内容及可见关联，宽度和画布状态由外层分栏负责。 */
export function StructureDetails({ comparison, selectedId, showImports, dark }: {
  comparison: StructureComparison; selectedId?: string; showImports: boolean; dark: boolean;
}) {
  const { t } = useTranslation("structure");
  const details = selectedId === undefined ? undefined : objectDetails(comparison, selectedId);
  const selected = details?.after ?? details?.before;
  const referenceChanges = details?.difference?.references.filter((change) => isReferenceVisible(change.value, { showImports })) ?? [];
  const selectedReferences = selected?.references.filter((reference) => isReferenceVisible(reference, { showImports })) ?? [];
  return <aside className="structure-details flex h-full min-w-0 flex-col bg-muted/20" aria-label={t("detailsPanel")}>
    <header className="shrink-0 border-b bg-background/95 px-3 py-3">
      <h3 className="text-sm font-semibold">{t("detailsTitle")}</h3>
    </header>
    <div key={selectedId} className="min-h-0 flex-1 overflow-auto p-3" data-structure-detail-content>
      {selectedId === undefined && <p className="py-5 text-sm text-muted-foreground">{t("selectObject")}</p>}
      {selected && details && <>
        <h3 className="break-words font-mono text-sm font-medium">{selected.name ?? selected.signature ?? selected.nativeKind}</h3>
        <p className="mt-1 text-xs text-muted-foreground">{selected.language} · {selected.nativeKind} · {t(`status.${details.difference?.content ?? (details.difference ? "relations" : "unchanged")}`)}</p>
        <SourceDifference before={details.before} after={details.after} dark={dark} />
        {(["before", "after"] as const).map((side) => {
          const extensions = details[side]?.extensions;
          return extensions && Object.keys(extensions).length > 0 && <section key={side} className="mt-4 min-w-0">
            <h4 className="mb-2 text-xs font-medium">{t(side)} · {t("languageDetails")}</h4>
            <pre className="max-h-60 overflow-auto rounded-md border p-3 text-[11px]">{JSON.stringify(extensions, null, 2)}</pre>
          </section>;
        })}
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

/** 原文包含完整声明和实现；缺失源码与对象不存在分别表达，避免制造虚假的增删。 */
function SourceDifference({ before, after, dark }: { before?: CodeNode; after?: CodeNode; dark: boolean }) {
  const { t } = useTranslation("structure");
  const missingSource = (before && before.source == null) || (after && after.source == null);
  return <section className="mt-5 min-w-0" aria-label={t("sourceDiff")}>
    <div className="mb-2 flex flex-wrap items-center justify-between gap-2 text-xs">
      <h4 className="font-medium">{t("sourceDiff")}</h4>
      <div className="flex flex-wrap gap-x-3 gap-y-1">
        <span className="text-red-700 dark:text-red-300">− {t("before")}</span>
        <span className="text-emerald-700 dark:text-emerald-300">+ {t("after")}</span>
      </div>
    </div>
    {(!before || !after) && <p className="mb-2 text-xs text-muted-foreground">{t(!before ? "before" : "after")} · <span>{t("absent")}</span></p>}
    {missingSource ? <p className="rounded-md border p-3 text-xs text-muted-foreground">{t("noSource")}</p> : <StructureCodeDiff
      original={before?.source ?? ""} modified={after?.source ?? ""}
      originalLanguage={before?.language ?? after?.language ?? "plaintext"}
      modifiedLanguage={after?.language ?? before?.language ?? "plaintext"} dark={dark}
    />}
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
