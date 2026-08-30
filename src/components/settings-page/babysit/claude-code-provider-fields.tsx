import { useState } from "react";

import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";

import { ProviderExecutableFields, type InstalledProviderFieldsProps } from "./provider-executable-fields";

const claudeModels = [
  { value: "", label: "Default" }, { value: "best", label: "Best（使用最强的可用模型）" },
  { value: "fable", label: "Fable" }, { value: "opus", label: "Opus" },
  { value: "sonnet", label: "Sonnet" }, { value: "haiku", label: "Haiku" }, { value: "custom", label: "Custom" },
];
const claudeEfforts = ["low", "medium", "high", "xhigh", "max"];

/** 呈现 Claude Code 离线 alias、自定义模型与受模型约束的 effort。 */
export function ClaudeCodeProviderFields({ preference, saving, onUpdate, onChooseExecutable }: InstalledProviderFieldsProps) {
  const customPreference = preference.model !== null && !claudeModels.some((item) => item.value === preference.model);
  const [customEditing, setCustomEditing] = useState(customPreference);
  const [customModel, setCustomModel] = useState(customPreference ? preference.model ?? "" : "");
  return <div className="grid min-w-0 gap-5" data-testid="claude-code-preferences">
    <ProviderExecutableFields label="Claude Code" preference={preference} saving={saving} onUpdate={onUpdate} onChooseExecutable={onChooseExecutable} />
    <label className="grid max-w-md gap-2 text-sm font-medium">模型<Select value={customEditing || customPreference ? "custom" : preference.model ?? "__default"} disabled={saving} onValueChange={(value) => {
      if (!value) return;
      if (value === "custom") { setCustomModel(customPreference ? preference.model ?? "" : ""); setCustomEditing(true); return; }
      setCustomEditing(false);
      const model = value === "__default" ? null : value;
      void onUpdate({ ...preference, model, reasoningEffort: model === "haiku" ? null : preference.reasoningEffort });
    }}><SelectTrigger aria-label="模型" className="h-11 w-full rounded-xl bg-background px-4"><SelectValue>{customEditing || customPreference ? "Custom" : claudeModels.find((item) => item.value === (preference.model ?? ""))?.label}</SelectValue></SelectTrigger><SelectContent align="start" alignItemWithTrigger={false}>{claudeModels.map((item) => <SelectItem key={item.value || "default"} value={item.value || "__default"}>{item.label}</SelectItem>)}</SelectContent></Select></label>
    {(customEditing || customPreference) && <label className="grid max-w-md gap-2 text-sm font-medium">自定义模型<input aria-label="自定义模型" className="h-11 rounded-xl border bg-background px-4" value={customModel} onChange={(event) => setCustomModel(event.target.value)} onBlur={() => customModel.trim() && void onUpdate({ ...preference, model: customModel.trim() })} /></label>}
    <label className="grid max-w-md gap-2 text-sm font-medium">思考强度<Select value={preference.reasoningEffort ?? "default"} disabled={saving || preference.model === "haiku"} onValueChange={(value) => value && void onUpdate({ ...preference, reasoningEffort: value === "default" ? null : value })}><SelectTrigger aria-label="思考强度" className="h-11 w-full rounded-xl bg-background px-4"><SelectValue>{preference.reasoningEffort === null ? "Default" : preference.reasoningEffort === "xhigh" ? "XHigh" : preference.reasoningEffort[0].toUpperCase() + preference.reasoningEffort.slice(1)}</SelectValue></SelectTrigger><SelectContent align="start" alignItemWithTrigger={false}><SelectItem value="default">Default</SelectItem>{preference.model !== "haiku" && claudeEfforts.map((effort) => <SelectItem key={effort} value={effort}>{effort === "xhigh" ? "XHigh" : effort[0].toUpperCase() + effort.slice(1)}</SelectItem>)}</SelectContent></Select></label>
  </div>;
}
