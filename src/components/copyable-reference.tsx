import { toast } from "sonner";

import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";

/** 可复制的分支或 revision 引用，悬停时提供完整值并确保键盘可访问。 */
export function CopyableReference({
  value,
  children,
  copiedLabel,
  copyFailedLabel,
  className,
}: {
  value: string;
  children: React.ReactNode;
  copiedLabel: string;
  copyFailedLabel: string;
  className?: string;
}) {
  /** 复制完整引用，并用非阻断 Toast 反馈结果。 */
  async function copyReference() {
    try {
      await navigator.clipboard.writeText(value);
      toast.success(copiedLabel);
    } catch {
      toast.error(copyFailedLabel);
    }
  }

  return <Tooltip>
    <TooltipTrigger render={<button type="button" className={className} onClick={() => void copyReference()} />}>
      {children}
    </TooltipTrigger>
    <TooltipContent className="max-w-xl break-all">{value}</TooltipContent>
  </Tooltip>;
}
