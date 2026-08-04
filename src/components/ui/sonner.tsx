import { Toaster as Sonner, type ToasterProps } from "sonner"
import {
  AlertCircleIcon,
  CancelCircleIcon,
  CheckmarkCircle02Icon,
  InformationCircleIcon,
} from "@hugeicons/core-free-icons"
import { Icon } from "@/components/ui/icon"
import { Spinner } from "@/components/ui/spinner"

/** 使用系统主题 token 渲染全局、非阻断的短暂反馈。 */
const Toaster = ({ ...props }: ToasterProps) => {
  return (
    <Sonner
      theme="system"
      className="toaster group"
      icons={{
        success: (
          <Icon icon={CheckmarkCircle02Icon} className="size-4" />
        ),
        info: (
          <Icon icon={InformationCircleIcon} className="size-4" />
        ),
        warning: (
          <Icon icon={AlertCircleIcon} className="size-4" />
        ),
        error: (
          <Icon icon={CancelCircleIcon} className="size-4" />
        ),
        loading: (
          <Spinner aria-hidden />
        ),
      }}
      style={
        {
          "--normal-bg": "var(--popover)",
          "--normal-text": "var(--popover-foreground)",
          "--normal-border": "var(--border)",
          "--border-radius": "var(--radius)",
        } as React.CSSProperties
      }
      toastOptions={{
        classNames: {
          toast: "cn-toast",
        },
      }}
      {...props}
    />
  )
}

export { Toaster }
