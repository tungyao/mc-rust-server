import { readFileSync } from 'node:fs';
import { fileURLToPath, URL } from 'node:url';
import { checkOffline } from './offline-check.mjs';

const target = process.argv[2]
  || fileURLToPath(new URL('../../crates/mc-server/src/admin_page.html', import.meta.url));
const problems = checkOffline(readFileSync(target, 'utf8'));
if (problems.length) {
  console.error(`${target} 离线检查失败：`, problems.join(', '));
  process.exit(1);
}
console.log(`${target} 离线检查通过`);
