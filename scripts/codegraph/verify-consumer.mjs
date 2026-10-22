import fs from 'node:fs/promises';
import path from 'node:path';
import { spawn } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { ids, targets } from './download-packs.mjs';

const repository = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
const inventory = JSON.parse(await fs.readFile(path.join(repository, 'resources/codegraph-consumer-acceptance.json')));
const retainedDiffNames = inventory.diffTests.map(item => `${item.new.replace('src-tauri/src/', '').replace(/\.rs$/, '').replaceAll('/', '::')}::${item.test}`);

/** 受控候选只带公共六字段；显式选择缺包必须失败。 */
export function validateCandidates(values, target, language = 'all') {
  if (!Array.isArray(values) || (language !== 'all' && !ids.includes(language))) throw new Error('Invalid CodeGraph candidate selection');
  const seen = new Set();
  for (const item of values) {
    if (Object.keys(item).sort().join() !== ['id','version','engineVersion','protocol','target','root'].sort().join() || !ids.includes(item.id) || seen.has(item.id) || item.version !== '0.1.0' || item.engineVersion !== '0.1.0' || item.protocol !== 'codegraph.analyzer/1' || item.target !== target || !path.isAbsolute(item.root)) throw new Error('Invalid public CodeGraph candidate identity');
    seen.add(item.id);
  }
  for (const id of language === 'all' ? ids : [language]) if (!seen.has(id)) throw new Error(`Missing CodeGraph candidate: ${id}`);
  return values;
}

/** 每组命令只运行 Narrant 消费验收，helper 源码与工厂验收归 CodeGraph CI。 */
export function verificationCommands(language = 'all', platform = process.platform) {
  if (language !== 'all' && !ids.includes(language)) throw new Error('Unknown CodeGraph language');
  const selected = language === 'all' ? ids : [language];
  const command = (filter, integration = false, ignored = true) => ({ executable: 'bash', requiredTests: integration ? [] : retainedDiffNames.filter(name => name.includes(filter)), args: ['scripts/cargo-agent.sh','test','--locked','--manifest-path','src-tauri/Cargo.toml', ...(integration ? ['--test','codegraph_graphs'] : ['--lib']), filter, '--', ...(ignored ? ['--ignored'] : []), '--nocapture','--test-threads=1'] });
  const commands = [command('child_process::', false, false), command('codegraph_client::tests::transport::', false, false), command('structure_review::code_graph::', false, false), command('language_support::', false, false), command('structure_review::application::tests::', false, false)];
  const suites = {
    rust: ['codegraph_client::tests::rust_graph::','codegraph_client::tests::rust_syntax::','codegraph_client::tests::rust_production::',...(platform === 'win32' ? [] : ['codegraph_client::tests::rust_lifecycle::']),'real_installed_rust_pack_analyzes_with_existing_toolchain','real_rust_pack_flows_through_review_lifecycle_to_dto'],
    'typescript-javascript': ['codegraph_client::tests::typescript_graph::','real_installed_typescript_pack_analyzes_without_development_tools','real_typescript_javascript_graphs_flow_through_review_lifecycle_to_dto',...(platform === 'win32' ? [] : ['real_cli_exit_and_cancellation_preserve_workspace_and_pack_barriers'])],
    python: ['codegraph_client::tests::python_graph::','codegraph_client::tests::python_native::','real_installed_python_pack_analyzes_without_user_python','native_python_mixed_review_uses_two_shared_workspaces','real_python_pack_flows_through_review_lifecycle_to_dto'],
    java: [], go: [], csharp: [], 'c-cpp': [],
  };
  for (const id of selected) for (const filter of suites[id]) commands.push(command(filter));
  commands.push(command('real_codegraph_packs_install_and_analyze_through_public_cli'));
  const filters = { rust:'rust_', 'typescript-javascript':'typescript_', python:'python_', java:'java_', go:'go_', csharp:'csharp_', 'c-cpp':'clang_' };
  commands.push(command(language === 'all' ? '' : filters[language], true));
  return commands;
}

/** 成功摘要之外，38 项保留差异测试还必须逐项出现命名成功行。 */
export function validTestOutput(output, requiredTests = []) {
  const passed = new Set(output.split(/\r?\n/).flatMap(line => /^test ([\w:]+) \.\.\. ok$/.exec(line)?.[1] ?? []));
  return /test result: ok\. [1-9]\d* passed;/.test(output) && requiredTests.every(name => passed.has(name));
}

/** 透传真实命令输出，并拒绝失败、零测试或缺失保留的命名测试结果。 */
async function execute(command, environment) {
  console.log(`CodeGraph consumer: ${command.executable} ${command.args.join(' ')}`);
  const child = spawn(command.executable, command.args, { cwd: repository, env: environment, stdio: ['ignore','pipe','inherit'] });
  let tail = '';
  child.stdout.on('data', data => { process.stdout.write(data); tail = (tail + data.toString()).slice(-1024 * 1024); });
  await new Promise((resolve, reject) => { child.once('error', reject); child.once('close', code => code === 0 && validTestOutput(tail, command.requiredTests) ? resolve() : reject(new Error(`CodeGraph consumer test failed or ran zero tests (${code})`))); });
}

/** 所有 Cargo 组串行执行；本机候选明确独立于真实发布下载证据。 */
export async function verifyConsumer(artifacts, candidatesFile, language = 'all') {
  const target = targets[`${process.platform}-${process.arch}`];
  if (!target) throw new Error('Unsupported CodeGraph target');
  artifacts = path.resolve(artifacts);
  candidatesFile = path.resolve(candidatesFile);
  const candidates = validateCandidates(JSON.parse(await fs.readFile(candidatesFile)), target, language);
  for (const item of candidates) await fs.access(item.root);
  const environment = { ...process.env, NARRANT_TEST_NODE: process.execPath, NARRANT_CODEGRAPH_ARTIFACTS: artifacts, NARRANT_CODEGRAPH_PACKS: candidatesFile, NARRANT_CODEGRAPH_LANGUAGE: language };
  for (const [id, key] of [['rust','NARRANT_RUST_PACK'], ['typescript-javascript','NARRANT_TYPESCRIPT_PACK'], ['python','NARRANT_PYTHON_PACK']]) {
    const candidate = candidates.find(item => item.id === id);
    if (candidate) environment[key] = candidate.root;
  }
  for (const command of verificationCommands(language)) await execute(command, environment);
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const artifacts = process.argv[2] ?? 'codegraph-packs';
  const option = name => process.argv.includes(name) ? process.argv[process.argv.indexOf(name) + 1] : undefined;
  await verifyConsumer(artifacts, option('--candidates') ?? path.join(artifacts,'candidates.json'), option('--language') ?? 'all');
}
