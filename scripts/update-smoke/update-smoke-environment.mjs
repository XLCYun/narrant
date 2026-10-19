const SYSTEM_VARIABLES = [
  'PATH', 'SystemRoot', 'SystemDrive', 'windir', 'ComSpec', 'PATHEXT', 'PSModulePath', 'OS',
  'HOME', 'USERPROFILE', 'USERNAME', 'HOMEDRIVE', 'HOMEPATH', 'LOCALAPPDATA', 'APPDATA',
  'ProgramData', 'ProgramFiles', 'ProgramFiles(x86)', 'TEMP', 'TMP', 'TMPDIR', 'LANG', 'LC_ALL',
];

/** 系统工具和正式应用共用公共环境；调用方显式追加 driver 所需的非 secret 字段。 */
export function selectSystemEnvironment(environment, extraNames = []) {
  return Object.fromEntries([...SYSTEM_VARIABLES, ...extraNames]
    .flatMap(name => environment[name] === undefined ? [] : [[name, environment[name]]]));
}
