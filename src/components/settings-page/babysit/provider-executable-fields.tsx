import { CheckmarkCircle02Icon } from "@hugeicons/core-free-icons";

import { Button } from "@/components/ui/button";
import { Icon } from "@/components/ui/icon";
import type { BabysitProviderPreference } from "@/settings/application-settings";

/** Installed Provider 编辑器共享的稳定 interface。 */
export type InstalledProviderFieldsProps = {
  preference: BabysitProviderPreference;
  saving: boolean;
  onUpdate(preference: BabysitProviderPreference, revision?: string | null): Promise<void>;
  onChooseExecutable(): Promise<void>;
};

/** 呈现 executable 值、原生选择与恢复自动发现操作。 */
export function ProviderExecutableFields({ label, preference, saving, onUpdate, onChooseExecutable }: InstalledProviderFieldsProps & { label: string }) {
  const automaticallyDiscovered = preference.executable === null;
  return <div className="min-w-0 rounded-2xl border bg-card p-4 sm:p-5">
    <h3 className="font-heading text-lg font-semibold">{label} 设置</h3>
    <p className="mt-5 text-sm font-medium">可执行文件</p>
    <div className="mt-2 flex min-w-0 flex-col gap-3 rounded-xl border bg-background/70 p-4 sm:flex-row sm:items-center">
      <div className="flex min-w-0 flex-1 items-center gap-3">
        <span className="flex size-9 shrink-0 items-center justify-center rounded-full bg-primary/10 text-primary" aria-hidden="true"><Icon icon={CheckmarkCircle02Icon} className="size-5" /></span>
        <div className="min-w-0">
          <p className="truncate font-mono text-sm" title={preference.executable ?? "由系统自动定位"}>{preference.executable ?? "由系统自动定位"}</p>
          <p className="mt-1 text-xs font-medium text-primary">{automaticallyDiscovered ? "自动发现" : "已选择"}</p>
        </div>
      </div>
      <Button variant="outline" className="self-start rounded-lg sm:self-auto" disabled={saving} onClick={() => void onChooseExecutable()}>更改</Button>
    </div>
    {preference.executable && <div className="mt-3 flex justify-end">
      <Button variant="ghost" size="sm" disabled={saving} onClick={() => void onUpdate({ ...preference, executable: null })}>恢复自动发现</Button>
    </div>}
  </div>;
}
