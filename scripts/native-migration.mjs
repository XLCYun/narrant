/** 已接通生产工厂且具备真实验收的集合；核心探针不能进入这个表。 */
export const migratedNativePacks = Object.freeze({
  rust: { helper: 'rust', toolchains: [] },
  python: { helper: 'python', toolchains: [] },
  'typescript-javascript': { helper: 'typescript', toolchains: [] },
  java: { helper: 'java', toolchains: ['jdk25', 'jdk17', 'jdk8'] },
  go: { helper: 'go', toolchains: ['go', 'go-target'] },
  csharp: { helper: 'csharp', toolchains: ['dotnet', 'dotnet-ref8', 'dotnet-ref9'] },
  'c-cpp': { helper: 'clang', toolchains: ['llvm'] },
});

/** 显式选择已完成语言；拼写错误和未完成迁移必须失败，不能变成空验收。 */
export function selectedNativePacks(language = 'all') {
  if (language === 'all') return Object.keys(migratedNativePacks);
  if (!Object.hasOwn(migratedNativePacks, language)) throw new Error(`Native helper migration not enabled: ${language}`);
  return [language];
}

/** 只安装所选语言需要的固定工具，完整清单验证仍覆盖双平台。 */
export function selectedToolchainNames(language = 'all') {
  return selectedNativePacks(language).flatMap(id => migratedNativePacks[id].toolchains);
}
