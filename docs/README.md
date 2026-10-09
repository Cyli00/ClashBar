# ClashBar 文档站

Next.js + Fumadocs站点，为唯一维护的Rust + Tauri Windows客户端提供使用说明。

```bash
pnpm install --frozen-lockfile
pnpm types:check
pnpm build
pnpm dev
```

内容位于 `content/docs/`，首页在 `src/app/(home)/page.tsx`，共享仓库链接在 `src/lib/shared.ts`。`src/app/global.css` 和页面现有Tailwind类拥有样式。

客户端发布链接指向 `Cyli00/ClashBar`。旧上游版本记录可作为历史保留，但不能用于宣称当前Windows包体、平台兼容或原生验收通过。`public/clashbar-light.png` 和 `clashbar-black.png` 是原版布局参考图，首页明确标注来源性质。

构建时运行 `scripts/parse-changelog.mjs`，从仓库 `CHANGELOG.md` 生成版本数据。Cloudflare部署仅在明确发布文档站时执行；本地类型检查和构建不会发布。
