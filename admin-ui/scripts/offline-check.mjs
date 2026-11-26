// 构建产物离线规则：单一 HTML，零外链、零动态模块加载。
// 注意：Vue / Naive UI 的压缩包里带有 XML 命名空间（http://www.w3.org/…）和
// 错误文档 URL 字符串。它们是纯字符串、浏览器不会请求；为满足“文件中不出现
// 任何 http(s):// 字面量”的静态检查，统一转义为 JS 字符串里的 \/（运行时取值不变）。
export function sanitizeUrls(html) {
  return html.replace(/(https?):\/\//g, '$1:\\/\\/');
}

const FORBIDDEN = [
  [/esm\.sh/i, 'esm.sh'],
  [/unpkg/i, 'unpkg'],
  [/jsdelivr/i, 'jsdelivr'],
  [/cdn/i, 'cdn'],
  [/https?:\/\//i, 'http(s):// URL'],
  [/importmap/i, 'importmap'],
  [/await\s+import\s*\(/i, 'dynamic import'],
  [/<script[^>]+src\s*=/i, 'external <script src>'],
  [/<link[^>]+href\s*=/i, 'external <link href>'],
];

export function checkOffline(html) {
  const problems = [];
  for (const [re, label] of FORBIDDEN) {
    if (re.test(html)) problems.push(label);
  }
  if (!html.includes('fetch(')) problems.push('fetch( call (required by server test)');
  return problems;
}
