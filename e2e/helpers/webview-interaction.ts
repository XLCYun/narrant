/** 为 embedded Tauri WebDriver 点击补充业务上下文，不隐藏或自动重试原始失败。 */
export async function clickWebView(element: WebdriverIO.Element, context: string) {
  try {
    await element.click();
  } catch (reason) {
    throw new Error(`${context}：${String(reason)}`, { cause: reason });
  }
}
