// 官网视觉验收截图：静态服务 site/out，截取关键页面（中英、桌面/移动宽度）。
// 用法：node app/scripts/shot-site.mjs [outDir]
import { chromium } from "playwright";
import { createServer } from "node:http";
import { readFile } from "node:fs/promises";
import { existsSync, mkdirSync } from "node:fs";
import { extname, join, resolve } from "node:path";

const root = resolve("site/out");
const outDir = resolve(process.argv[2] ?? "target/tmp/site-shots");
mkdirSync(outDir, { recursive: true });

const MIME = {
  ".html": "text/html", ".css": "text/css", ".js": "text/javascript",
  ".png": "image/png", ".svg": "image/svg+xml", ".webp": "image/webp",
  ".ico": "image/x-icon", ".woff2": "font/woff2", ".xml": "text/xml", ".txt": "text/plain",
};

const server = createServer(async (req, res) => {
  const url = decodeURIComponent(new URL(req.url, "http://x").pathname);
  let file = join(root, url);
  if (!existsSync(file) || url.endsWith("/")) file = join(file, "index.html");
  if (!existsSync(file) && !extname(url)) file = join(root, url + ".html");
  try {
    const data = await readFile(file);
    res.writeHead(200, { "content-type": MIME[extname(file)] ?? "application/octet-stream" });
    res.end(data);
  } catch {
    res.writeHead(404);
    res.end("nf");
  }
});
await new Promise((r) => server.listen(0, r));
const port = server.address().port;

const pages = [
  ["home-zh", "/", 1440, 900],
  ["home-en", "/en/", 1440, 900],
  ["features-zh", "/features/", 1440, 900],
  ["features-en", "/en/features/", 1440, 900],
  ["download-zh", "/download/", 1440, 900],
  ["docs-zh", "/docs/", 1440, 900],
  ["faq-zh", "/faq/", 1440, 900],
  ["home-mobile", "/", 390, 844],
];

const browser = await chromium.launch();
for (const [name, path, w, h] of pages) {
  const page = await browser.newPage({ viewport: { width: w, height: h } });
  await page.goto(`http://127.0.0.1:${port}${path}`, { waitUntil: "networkidle" });
  // fullPage 截图不会触发 IntersectionObserver，强制 reveal 元素可见，避免截到假空白
  await page.addStyleTag({ content: ".reveal{opacity:1 !important;transform:none !important}" });
  await page.waitForTimeout(500);
  await page.screenshot({ path: join(outDir, `${name}.png`), fullPage: true });
  await page.close();
  console.log("shot", name);
}
await browser.close();
server.close();
console.log("done ->", outDir);
