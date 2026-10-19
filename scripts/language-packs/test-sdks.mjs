/** 被分析项目的固定 SDK 夹具；不包含 helper 的编译器或打包工具。 */
const projectSdks = Object.freeze({
  rust: [], python: [], 'typescript-javascript': [],
  java: ['jdk25', 'jdk17', 'jdk8'],
  go: ['go', 'go-target'],
  csharp: ['dotnet', 'dotnet-ref8', 'dotnet-ref9'],
  'c-cpp': [],
});

/** 显式选择的语言必须存在；空集合表示该语言无需额外 SDK 下载。 */
export function selectedToolchainNames(language = 'all') {
  if (language === 'all') return Object.values(projectSdks).flat();
  if (!Object.hasOwn(projectSdks, language)) throw new Error(`Unknown CodeGraph language: ${language}`);
  return [...projectSdks[language]];
}
