# mc-admin-ui（完全离线的管理面板前端）

`crates/mc-server/src/admin_page.html` 的**源码**。构建产物是一个自包含的
`dist/index.html`，由 `npm run build` 自动复制到 `../crates/mc-server/src/admin_page.html`，
后端仍用 `include_str!` + `GET /` 提供页面，**零 Rust 改动、零运行时文件依赖、零外网请求**。

## 依赖（仅构建期，需要 npm 联网安装）

- `vue` 3.5.13
- `naive-ui` 2.41.0
- `vite` 5.4.21（构建工具）
- `vite-plugin-singlefile` 2.3.4（把 JS/CSS 全部内联进单个 HTML）

运行期（打开面板的浏览器）不需要任何 npm 包、CDN、外部字体或图片。
`package-lock.json` 已提交，可复现构建。

## 构建

```bash
cd admin-ui
npm ci
npm run build          # vite build + scripts/postbuild.mjs
npm run check:offline  # 单独校验已提交的 admin_page.html
```

构建配置要点（`vite.config.js`）：

- `resolve.alias.vue → vue/dist/vue.esm-bundler.js`：模板以 JS 字符串编写，需要运行时编译器；
  alias 对 Naive UI 同样生效，保证 Vue 只有一个实例。
- `build.cssCodeSplit: false` + 超大 `assetsInlineLimit` +
  `vite-plugin-singlefile` + `inlineDynamicImports`：输出单个 `dist/index.html`。
- 构建后脚本会把压缩包内 Vue/Naive UI 的字符串字面量中的 `http://` / `https://`
  （XML 命名空间、错误文档地址等，**不会被浏览器请求**）转义成 `http:\/\/`，
  运行时取值不变，从而保证 HTML 里没有任何 URL 字面量、静态检查可核验零外链。
  脚本同时断言：无 `importmap`、无 `await import(`、无 `<script src>`/`<link href>`、
  含 `fetch(`（服务端单测要求）；任一不满足则构建失败。

## 与后端的契约（保持不变）

7 个 API：`GET /api/online`、`/api/world`、`/api/items`、`/api/inventory?username=…`，
`POST /api/give`、`/api/kick`、`/api/save`；broadcast 仍是 501，前端只展示说明、不发包。
5s 世界状态轮询、2s 在线列表轮询；1658 物品筛选/发放（中文名/分类）、46 槽背包展示。
