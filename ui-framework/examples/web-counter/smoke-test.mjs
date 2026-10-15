// Browser smoke test: loads the page in headless Chromium, clicks the
// canvas-rendered "+" button and checks the Rust state and the pixels.
// Usage: node smoke-test.mjs  (after building pkg/, see README)
import { createRequire } from "node:module";
import { createServer } from "node:http";
import { readFile } from "node:fs/promises";
import { extname, join, normalize } from "node:path";
import { fileURLToPath } from "node:url";

// Resolve playwright via require() so NODE_PATH / global installs work too.
const { chromium } = createRequire(import.meta.url)("playwright");
const root = fileURLToPath(new URL(".", import.meta.url));
const types = { ".html": "text/html", ".js": "text/javascript", ".wasm": "application/wasm" };
const server = createServer(async (req, res) => {
  const path = normalize(join(root, decodeURIComponent(new URL(req.url, "http://x").pathname)));
  if (!path.startsWith(root)) { res.writeHead(403).end(); return; }
  try {
    const file = path.endsWith("/") ? join(path, "index.html") : path;
    const body = await readFile(file);
    res.writeHead(200, { "content-type": types[extname(file)] ?? "application/octet-stream" });
    res.end(body);
  } catch { res.writeHead(404).end(); }
}).listen(0, "127.0.0.1");
await new Promise((r) => server.once("listening", r));
const url = `http://127.0.0.1:${server.address().port}/`;

const browser = await chromium.launch({ executablePath: process.env.CHROMIUM_PATH || undefined });
let failed = false;
const check = (ok, msg) => { console.log(`${ok ? "ok  " : "FAIL"} ${msg}`); if (!ok) failed = true; };
try {
  const page = await browser.newPage({ deviceScaleFactor: 2 });
  const errors = [];
  page.on("pageerror", (e) => errors.push(String(e)));
  await page.goto(url);
  await page.waitForFunction(() => window.kiteReady === true, null, { timeout: 15000 });
  const canvas = await page.$("canvas");
  const box = await canvas.boundingBox();
  check(box.width === 320 && box.height === 200, `canvas is 320x200 CSS px (got ${box.width}x${box.height})`);
  const backing = await page.evaluate(() => [document.querySelector("canvas").width, document.querySelector("canvas").height]);
  check(backing[0] === 640 && backing[1] === 400, `HiDPI backing store 640x400 (got ${backing})`);
  check(await page.evaluate(() => window.kite.counter_value()) === 0, "counter starts at 0");
  const [x, y, w, h] = await page.evaluate(() => Array.from(window.kite.element_rect("plus")));
  for (let i = 0; i < 3; i++) await page.mouse.click(box.x + x + w / 2, box.y + y + h / 2);
  const value = await page.evaluate(() => window.kite.counter_value());
  check(value === 3, `three clicks on "+" give 3 (got ${value})`);
  await page.waitForTimeout(100);
  // The "+" button is primary (blue) — sample its center in the backing store.
  const px = await page.evaluate(([cx, cy]) => {
    const c = document.querySelector("canvas");
    return Array.from(c.getContext("2d").getImageData(cx * 2, cy * 2, 1, 1).data);
  }, [x + 4, y + h / 2]);
  check(px[2] > 200 && px[0] < 100, `primary button pixel is blue (got rgba ${px})`);
  await page.keyboard.press("Tab");
  await page.keyboard.press("Space");
  const afterKey = await page.evaluate(() => window.kite.counter_value());
  // Clicking "+" focused it; Tab wraps to "-" and Space activates it.
  check(afterKey === 2, `Tab focuses "-" and Space activates it (got ${afterKey})`);
  await page.screenshot({ path: join(root, "smoke-test.png") });
  check(errors.length === 0, `no page errors ${errors.join("; ")}`);
} finally {
  await browser.close();
  server.close();
}
process.exit(failed ? 1 : 0);
