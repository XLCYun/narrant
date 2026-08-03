import { Skeleton } from "@/components/ui/skeleton";
import { Spinner } from "@/components/ui/spinner";

/** 在 Diff 区域呈现单个 File Change 的读取进度。 */
export function FileChangeLoading({ label }: { label: string }) {
  return <div role="status" aria-label={label} aria-busy="true" className="space-y-3 p-4">
    <div className="flex items-center gap-2 text-sm text-muted-foreground">
      <Spinner aria-hidden /><span>{label}</span>
    </div>
    <Skeleton className="h-6 w-2/3" />
    <Skeleton className="h-[28rem] w-full" />
  </div>;
}
