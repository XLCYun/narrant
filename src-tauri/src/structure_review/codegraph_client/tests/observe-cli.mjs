// 生命周期测试只观察真实公共 CLI，不改写协议或引入模拟结果。
import fs from 'node:fs';
import { spawn } from 'node:child_process';
const child = spawn(process.argv[2], process.argv.slice(3), { stdio: ['pipe','pipe','pipe'] });
fs.writeFileSync(process.env.CODEGRAPH_OBSERVED_PID, String(child.pid));
process.stdin.pipe(child.stdin);
child.stdout.pipe(process.stdout);
child.stderr.pipe(process.stderr);
child.stdin.on('error', () => {});
child.on('error', error => { process.stderr.write(String(error)); process.exitCode = 1; });
child.on('close', code => process.exit(code ?? 1));
