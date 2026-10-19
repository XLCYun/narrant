import { verifyNativeAnalyzers } from './verify-native-analyzers.mjs';

// Python 复用正常 Native 验收，严格选择实际 helper、安装、Review 及公开图入口。
await verifyNativeAnalyzers(process.argv[2] ?? 'dist/language-support-packs', false, 'python');
