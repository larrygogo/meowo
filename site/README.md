# Meowo 官网

Meowo / 喵呜 的产品官网。Next.js 15（App Router）+ React 19 + TypeScript，包管理用 Bun。

## 本地开发

```bash
cd site
bun install
bun run dev      # http://localhost:3000
```

## 构建

```bash
bun run build    # 全站静态预渲染
bun run start    # 本地跑生产构建
```

## 部署到 Vercel

1. 代码 push 到 GitHub。
2. Vercel → **Add New → Project** → 选中 `meowo` 仓库。
3. **Root Directory 设为 `site`**（关键：仓库根是 Tauri 应用，别让 Vercel 构建整个仓库）。
4. Framework 会自动识别为 **Next.js**，Build/Output 保持默认即可。
5.（可选）环境变量加 `NEXT_PUBLIC_SITE_URL = https://你的域名`，用于社交分享图的绝对地址。
6. Deploy。之后每次 push 自动重新部署。

> ⚠️ 国内访问：`*.vercel.app` 默认域名会被墙。上线后请在 Settings → Domains 绑定自有域名；
> 现阶段更适合作为展示站 / 面向海外，若要国内稳定访问需走备案 + 国内 CDN。

## 页面

| 路由 | 文件 | 内容 |
|------|------|------|
| `/` | `app/page.tsx` | 首页：主张、产品图、特性、工作原理、CTA |
| `/features` | `app/features/page.tsx` | 功能详情：深入讲解 + CSS 示意面板 + 平台差异 |
| `/download` | `app/download/page.tsx` | 下载：平台卡、安装步骤、环境要求 |
| `/docs` | `app/docs/page.tsx` | 文档：工作原理、接入、手动挂 hooks、数据与配置 |
| `/changelog` | `app/changelog/page.tsx` | 更新日志（版本 / 日期取自真实 git tag） |
| `/faq` | `app/faq/page.tsx` | 常见问题（折叠面板） |

## 结构

```
site/
├── app/
│   ├── layout.tsx          # 全站 <Nav/> + <Footer/>、SEO / OG 元信息
│   ├── globals.css         # 设计系统（浅色高级风：白底 + 翡翠绿点缀）
│   └── <route>/page.tsx    # 各页面（见上表）
├── components/
│   ├── Nav.tsx · Footer.tsx        # 全站导航与页脚（client 导航）
│   ├── ProductShowcase.tsx         # 深色贴纸产品图（窗口壳）
│   ├── FeatureGrid.tsx · CtaBand.tsx
│   ├── Reveal.tsx                  # 滚动进场动画
│   └── icons.tsx                   # 内联 SVG 图标
├── lib/site.ts             # 仓库 / Releases 链接、导航项常量
└── public/                 # logo.png、demo.webp
```

改文案：文本直接在对应 `app/**/page.tsx` 里改。导航项、下载与仓库地址集中在 `lib/site.ts`。

> 设计为浅色高级风（白底 + 细腻分割线 + 翡翠绿点缀）；产品截图为深色贴纸，在浅色页面上形成明暗对比。

## 更新演示图

`public/demo.webp` 来自仓库根的 `docs/images/demo.webp`（高保真 2× 动画 WebP）。要刷新到当前 UI：

```bash
cd ../app && bun run demo:webp     # 重新生成 docs/images/demo.webp（Playwright 2× 逐帧 + sharp 合成）
cp ../docs/images/demo.webp ../site/public/demo.webp
```
