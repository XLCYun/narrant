import { render } from "@testing-library/react"
import { ArrowRight01Icon } from "@hugeicons/core-free-icons"
import { describe, expect, it } from "vitest"
import { Icon } from "@/components/ui/icon"

describe("Icon", () => {
  it("默认继承文字颜色并使用统一线宽", () => {
    const { container } = render(<Icon icon={ArrowRight01Icon} />)
    const svg = container.querySelector("svg")
    const path = container.querySelector("path")

    expect(svg).toHaveAttribute("color", "currentColor")
    expect(path).toHaveAttribute("stroke-width", "2")
  })

  it("允许特殊场景显式覆盖线宽", () => {
    const { container } = render(<Icon icon={ArrowRight01Icon} strokeWidth={1.5} />)

    expect(container.querySelector("path")).toHaveAttribute("stroke-width", "1.5")
  })
})
