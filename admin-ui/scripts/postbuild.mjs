import { createHash } from 'node:crypto';
import { readFileSync, writeFileSync } from 'node:fs';
import { fileURLToPath, URL } from 'node:url';
import { checkOffline, sanitizeUrls } from './offline-check.mjs';

const dist = fileURLToPath(new URL('../dist/index.html', import.meta.url));
const target = fileURLToPath(new URL('../../crates/mc-server/src/admin_page.html', import.meta.url));

const html = sanitizeUrls(readFileSync(dist, 'utf8'));
const problems = checkOffline(html);
if (problems.length) {
  console.error('离线检查失败：', problems.join(', '));
  process.exit(1);
}
writeFileSync(target, html);
const sha = createHash('sha256').update(html).digest('hex');
console.log(`已写入 ${target}（${Buffer.byteLength(html)} 字节，sha256 ${sha}）`);
console.log('离线检查通过：单文件、无外链、无 importmap / 动态 import，含 fetch(');
