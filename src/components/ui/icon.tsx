import { forwardRef } from "react"
import { HugeiconsIcon, type HugeiconsIconProps } from "@hugeicons/react"

/** 统一图标组件透传的 Hugeicons 属性。 */
export type IconProps = HugeiconsIconProps

/**
 * 统一渲染界面操作与状态图标，集中保持颜色继承和线条重量。
 */
const Icon = forwardRef<SVGSVGElement, IconProps>(
  ({ color = "currentColor", strokeWidth = 2, ...props }, ref) => (
    <HugeiconsIcon
      ref={ref}
      color={color}
      strokeWidth={strokeWidth}
      {...props}
    />
  ),
)

Icon.displayName = "Icon"

export { Icon }
