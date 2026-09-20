import { test } from 'node:test';
import assert from 'node:assert/strict';
import { supportedNode, validateArchive } from '../setup-typescript-analyzer.mjs';

test('Node 最低版本是 TLS 的固定要求，不借用应用引擎', () => {
  for (const version of ['22.22.2', '24.18.0', '25.0.0']) assert.equal(supportedNode(version), true);
  for (const version of ['22.22.1', '22.13.0', '21.99.99', '24.0.0-beta', '24.0']) assert.equal(supportedNode(version), false);
});

test('归档允许 npm 的不同固定前缀，拒绝逃逸、链接和混合根', () => {
  validateArchive('package/\npackage/index.js\n', 'drwxr-xr-x package/\n-rw-r--r-- package/index.js\n');
  validateArchive('node/\nnode/index.d.ts\n', 'drwxr-xr-x node/\n-rw-r--r-- node/index.d.ts\n');
  for (const [names, listing] of [
    ['package/../../escape', '-rw-r--r-- package/../../escape'],
    ['/package/index', '-rw-r--r-- /package/index'],
    ['package/link', 'lrwxr-xr-x package/link -> /outside'],
    ['package/index\nother/index', '-rw-r--r-- package/index\n-rw-r--r-- other/index'],
    ['package/link', 'hrw-r--r-- package/link link to outside'],
  ]) assert.throws(() => validateArchive(names, listing));
});
