import { Loading03Icon } from "@hugeicons/core-free-icons"

import { Icon, type IconProps } from "@/components/ui/icon"
import { cn } from "@/lib/utils"

/**
 * 使用统一 HugeIcons 视觉呈现不确定时长的加载状态。
 */
function Spinner({ className, "aria-hidden": ariaHidden, ...props }: Omit<IconProps, "icon">) {
  return (
    <Icon
      icon={Loading03Icon}
      role={ariaHidden ? undefined : "status"}
      aria-label={ariaHidden ? undefined : "Loading"}
      aria-hidden={ariaHidden}
      className={cn("size-4 animate-spin", className)}
      {...props}
    />
  )
}

export { Spinner }
