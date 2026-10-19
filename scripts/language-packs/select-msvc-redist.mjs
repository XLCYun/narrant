import fs from 'node:fs/promises';
import path from 'node:path';
import { execFileSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

/** 只读取所选 CRT 目录中 DLL 的 Windows 版本资源，不加载或执行 DLL。 */
export function readWindowsDllVersions(directory) {
  const script = `$ErrorActionPreference='Stop'; $rows=@(Get-ChildItem -LiteralPath $env:NARRANT_REDIST_PROBE -Filter '*.dll' -File | ForEach-Object { $v=$_.VersionInfo; [pscustomobject]@{name=$_.Name; runtime_version=('{0}.{1}.{2}' -f $v.FileMajorPart,$v.FileMinorPart,$v.FileBuildPart); file_version=('{0}.{1}.{2}.{3}' -f $v.FileMajorPart,$v.FileMinorPart,$v.FileBuildPart,$v.FilePrivatePart)} }); ConvertTo-Json -InputObject $rows -Compress`;
  return JSON.parse(execFileSync(path.win32.join(process.env.SystemRoot, 'System32/WindowsPowerShell/v1.0/powershell.exe'), ['-NoProfile', '-NonInteractive', '-Command', script], { encoding: 'utf8', timeout: 15_000, maxBuffer: 128 * 1024, windowsHide: true, env: { ...process.env, NARRANT_REDIST_PROBE: directory } }));
}

/** 用 VS 自己的固定元数据选唯一目录，再验证运行库版本；目录版本不等于 DLL 版本。 */
export async function selectMsvcRedist({ visualStudio, expectedVersion, readVersions = readWindowsDllVersions, log = console.log }) {
  if (!path.isAbsolute(visualStudio) || !/^\d+\.\d+\.\d+$/.test(expectedVersion)) throw new Error('Invalid fixed MSVC runtime inputs');
  const root = path.join(visualStudio, 'VC/Redist/MSVC');
  const metadata = path.join(visualStudio, 'VC/Auxiliary/Build/Microsoft.VCRedistVersion.default.txt');
  let token;
  try {
    token = (await fs.readFile(metadata, 'utf8')).trim();
    log(`MSVC redist metadata: ${metadata} = ${JSON.stringify(token)}`);
    if (!/^(?:\d+\.\d+\.\d+(?:\.\d+)?|v143)$/.test(token)) throw new Error('Invalid VS redist metadata token');
    const selected = await fs.realpath(path.join(root, token, 'x64/Microsoft.VC143.CRT'));
    const relative = path.relative(await fs.realpath(root), selected);
    if (relative === '..' || relative.startsWith(`..${path.sep}`) || path.isAbsolute(relative)) throw new Error('MSVC redist directory escapes fixed VS root');
    const files = (await fs.readdir(selected, { withFileTypes: true })).filter(file => file.isFile() && file.name.toLowerCase().endsWith('.dll')).map(file => file.name);
    const versions = await readVersions(selected);
    log(`MSVC CRT directory: ${selected}; DLL versions: ${JSON.stringify(versions)}`);
    for (const required of ['msvcp140.dll', 'vcruntime140.dll', 'vcruntime140_1.dll']) if (!files.some(name => name.toLowerCase() === required)) throw new Error(`Missing required CRT DLL: ${required}`);
    if (!Array.isArray(versions) || versions.length !== files.length || new Set(versions.map(file => file.name?.toLowerCase())).size !== files.length) throw new Error('Incomplete MSVC DLL version evidence');
    for (const file of versions) {
      if (!files.some(name => name.toLowerCase() === file.name?.toLowerCase()) || file.runtime_version !== expectedVersion) throw new Error(`MSVC runtime version mismatch: ${file.name} = ${file.file_version}; expected ${expectedVersion}`);
    }
    return selected;
  } catch (error) {
    // 诊断只列固定 VS 根的直接目录，不从列表挑“最新”或改变运行库 pin。
    const directories = await fs.readdir(root, { withFileTypes: true }).then(entries => entries.filter(entry => entry.isDirectory()).map(entry => entry.name)).catch(() => []);
    log(`MSVC redist selection failed; metadata=${metadata}; token=${JSON.stringify(token ?? null)}; direct directories=${JSON.stringify(directories)}`);
    throw error;
  }
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const selected = await selectMsvcRedist({ visualStudio: process.argv[2], expectedVersion: process.argv[3] });
  if (!process.env.GITHUB_ENV) throw new Error('Missing GITHUB_ENV for selected CRT root');
  await fs.appendFile(process.env.GITHUB_ENV, `NARRANT_MSVC_REDIST=${selected}\n`);
}
