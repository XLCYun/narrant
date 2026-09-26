import { useTranslation } from "react-i18next";
import { objectDetails } from "@/structure-review/graph-presentation";
import type { CodeNode, StructureComparison } from "@/structure-review/types";
import type { DirectoryGroup } from "@/structure-review/directory-groups";
import { StructureCodeDiff } from "./structure-code-diff";

/** 呈现选中对象的基本信息，并将下方剩余空间全部留给源码差异。 */
export function StructureDetails({ comparison, selectedId, directory, dark }: {
  comparison: StructureComparison; selectedId?: string; directory?: DirectoryGroup; dark: boolean;
}) {
  const { t } = useTranslation("structure");
  const details = selectedId === undefined ? undefined : objectDetails(comparison, selectedId);
  const selected = details?.after ?? details?.before;
  return <aside className="structure-details flex h-full min-h-0 min-w-0 flex-col bg-muted/20" aria-label={t("detailsPanel")}>
    <header className="shrink-0 border-b bg-background/95 px-3 py-3">
      <h3 className="text-sm font-semibold">{t(directory ? "directoryDetails" : "detailsTitle")}</h3>
    </header>
    <div key={directory ? `directory:${directory.path}` : `object:${selectedId}`} className="flex min-h-0 flex-1 flex-col overflow-hidden p-3" data-structure-detail-content>
      {selectedId === undefined && !directory && <p className="py-5 text-sm text-muted-foreground">{t("selectObject")}</p>}
      {directory && <div className="min-h-0 overflow-auto">
        <h3 className="break-all font-mono text-sm font-medium">{directory.path}</h3>
        <h4 className="mb-2 mt-5 text-xs font-medium">{t("directoryFiles")}</h4>
        <ul className="space-y-2 break-all font-mono text-xs text-muted-foreground">{directory.files.map((file) =>
          <li key={file.objectId} title={file.path}>{file.path.slice(directory.path.length + 1)}</li>)}</ul>
      </div>}
      {selected && details && <>
        <div className="max-h-[40%] shrink-0 overflow-auto">
          <h3 className="break-words font-mono text-sm font-medium">{selected.name ?? selected.signature ?? selected.nativeKind}</h3>
          <p className="mt-1 text-xs text-muted-foreground">{selected.language} · {selected.nativeKind} · {t(`status.${details.difference?.content ?? (details.difference ? "relations" : "unchanged")}`)}</p>
          {(["before", "after"] as const).map((side) => {
            const extensions = details[side]?.extensions;
            return extensions && Object.keys(extensions).length > 0 && <section key={side} className="mt-4 min-w-0">
              <h4 className="mb-2 text-xs font-medium">{t(side)} · {t("languageDetails")}</h4>
              <pre className="max-h-60 overflow-auto rounded-md border p-3 text-[11px]">{JSON.stringify(extensions, null, 2)}</pre>
            </section>;
          })}
        </div>
        <SourceDifference before={details.before} after={details.after} dark={dark} />
      </>}
    </div>
  </aside>;
}

/** 原文包含完整声明和实现；缺失源码与对象不存在分别表达，避免制造虚假的增删。 */
function SourceDifference({ before, after, dark }: { before?: CodeNode; after?: CodeNode; dark: boolean }) {
  const { t } = useTranslation("structure");
  const missingSource = (before && before.source == null) || (after && after.source == null);
  return <section className="mt-5 flex min-h-0 min-w-0 flex-1 flex-col" aria-label={t("sourceDiff")}>
    <div className="mb-2 flex shrink-0 flex-wrap items-center justify-between gap-2 text-xs">
      <h4 className="font-medium">{t("sourceDiff")}</h4>
      <div className="flex flex-wrap gap-x-3 gap-y-1">
        <span className="text-red-700 dark:text-red-300">− {t("before")}</span>
        <span className="text-emerald-700 dark:text-emerald-300">+ {t("after")}</span>
      </div>
    </div>
    {(!before || !after) && <p className="mb-2 shrink-0 text-xs text-muted-foreground">{t(!before ? "before" : "after")} · <span>{t("absent")}</span></p>}
    {missingSource ? <p className="rounded-md border p-3 text-xs text-muted-foreground">{t("noSource")}</p> : <StructureCodeDiff
      original={before?.source ?? ""} modified={after?.source ?? ""}
      originalLanguage={before?.language ?? after?.language ?? "plaintext"}
      modifiedLanguage={after?.language ?? before?.language ?? "plaintext"} dark={dark}
    />}
  </section>;
}
