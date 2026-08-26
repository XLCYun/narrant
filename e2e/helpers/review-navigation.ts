/** 将 canonical ref 转成界面展示的精确短分支名。 */
function shortBranchName(ref: string) {
  return ref.replace(/^refs\/(heads|remotes\/[^/]+)\//, "");
}

/** 从当前可见 active sidebar item 读取 durable Review identity。 */
export async function activeReviewId() {
  for (const item of await $$("[data-review-id][aria-current='page']")) {
    if (!(await item.isDisplayed())) continue;
    const reviewId = await item.getAttribute("data-review-id");
    if (reviewId) return reviewId;
  }
  throw new Error("当前视图没有可见的 active Review identity");
}

/** 通过真实 shadcn/Base UI Combobox 精确选择 canonical ref。 */
export async function selectRef(label: "Base Ref" | "Head Ref", canonicalRef: string) {
  const triggers = await $$("[role='combobox']");
  const trigger = triggers[label === "Head Ref" ? 0 : 1];
  if (!trigger) throw new Error(`分支选择器数量不足：${triggers.length}`);
  await trigger.waitForDisplayed();
  if (await trigger.isClickable()) await trigger.click();
  else await browser.execute((index) => document.querySelectorAll<HTMLElement>("[role='combobox']")[index]?.click(), label === "Head Ref" ? 0 : 1);
  const branch = shortBranchName(canonicalRef);
  const search = await $("input[aria-label^='搜索分支'], input[aria-label^='Search branches']");
  await search.waitForDisplayed();
  await search.setValue(branch);
  const exactOption = `[role='option'][aria-label='${canonicalRef}']`;
  await browser.waitUntil(async () => {
    for (const option of await $$(exactOption)) if (await option.isDisplayed()) return true;
    return false;
  });
  for (const option of await $$(exactOption)) {
    if (!(await option.isDisplayed())) continue;
    await option.waitForClickable();
    await option.click();
    await browser.waitUntil(async () => browser.execute(
      (element) => element.querySelector(".font-mono")?.textContent?.trim(),
      trigger,
    ).then((selected) => selected === branch));
    return;
  }
  throw new Error(`找不到 ${label}: ${canonicalRef}`);
}

/** 从持久化 navigation 精确定位一个 Branch Selection 的 durable identity。 */
export async function reviewIdForSelection(baseRef: string, headRef: string) {
  const navigation = await browser.tauri.execute(
    ({ core }) => core.invoke<{
      recentReviews: Array<{ review: { reviewId: string; selection: { kind: string; baseRef: string; headRef: string } } }>;
    }>("load_repository_navigation"),
  );
  const review = navigation.recentReviews.find((candidate) => (
    candidate.review.selection.kind === "branch"
    && candidate.review.selection.baseRef === baseRef
    && candidate.review.selection.headRef === headRef
  ));
  if (!review) throw new Error(`没有持久化 Branch Review：${baseRef} → ${headRef}`);
  return review.review.reviewId;
}

/** 确保 desktop/offcanvas Sidebar 展开，并返回当前可见品牌按钮。 */
export async function visibleBrandButton() {
  const findBrand = async () => {
    for (const brand of await $$("button[aria-label='Narrant'], button[aria-label='纳言']")) {
      if (await brand.isDisplayed()) return brand;
    }
  };
  let brand = await findBrand();
  if (brand) return brand;
  await browser.keys("Escape");
  let trigger: WebdriverIO.Element | undefined;
  await browser.waitUntil(async () => {
    for (const candidate of await $$('[data-slot="sidebar-trigger"]')) {
      if (!(await candidate.isDisplayed())) continue;
      trigger = candidate;
      return true;
    }
    return false;
  }, { timeoutMsg: "没有可见的 Sidebar trigger" });
  if (!trigger) throw new Error("没有可见的 Sidebar trigger");
  await trigger.click();
  await browser.waitUntil(async () => Boolean(await findBrand()));
  brand = await findBrand();
  if (!brand) throw new Error("Sidebar 展开后仍没有可见品牌按钮");
  return brand;
}

/** 通过当前可见的 desktop/offcanvas Sidebar 返回 Home。 */
export async function navigateHome() {
  const brand = await visibleBrandButton();
  await brand.click();
  await browser.waitUntil(async () => {
    if (await $("main [aria-label='最近审核'], main [aria-label='Recent reviews']").isDisplayed()) return true;
    return await $("main").$("button=打开仓库文件夹").isDisplayed()
      || await $("main").$("button=Open repository folder").isDisplayed();
  });
}
