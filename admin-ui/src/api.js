// 与旧页面一致的极简 fetch 封装：API 路径与响应形状保持不变。
export async function jget(path) {
  const r = await fetch(path);
  return await r.json();
}

export async function jpost(path, body) {
  const r = await fetch(path, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(body),
  });
  return { status: r.status, body: await r.json() };
}

export function matchQ(it, q) {
  q = (q || '').trim().toLowerCase();
  if (!q) return true;
  return (it.name_zh || '').toLowerCase().includes(q)
    || (it.name || '').toLowerCase().includes(q)
    || String(it.id) === q;
}

// 完整清单 1658 项，网格最多渲染前 240 项（与旧页面一致）。
export const ITEM_CAP = 240;
