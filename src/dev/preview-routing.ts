/**
 * 解析预览场景；未知场景统一回退到组件声明的第一个场景。
 *
 * Preview 组件不必都声明名为 `default` 的场景，因此组件切换和旧书签都不能把
 * `default` 当作隐式存在的键。
 */
export function resolvePreviewScenarioKey(
  scenarios: Readonly<Record<string, unknown>>,
  requestedScenario: string,
): string {
  if (Object.hasOwn(scenarios, requestedScenario)) return requestedScenario;
  const firstScenario = Object.keys(scenarios)[0];
  if (!firstScenario) throw new Error("预览组件必须至少声明一个场景。");
  return firstScenario;
}
