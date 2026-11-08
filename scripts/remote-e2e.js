// End-to-end check of the phone web client (see README "开发与测试").
// Usage: cargo run -p cb-ui --example remote_host -- url.txt 25 &  then  node scripts/remote-e2e.js url.txt phone.png
// Drives the phone web client in a mobile-emulated Chromium.
const { chromium, devices } = require('playwright');
const fs = require('fs');
(async () => {
  const url = fs.readFileSync(process.argv[2], 'utf8').trim();
  const browser = await chromium.launch();
  const ctx = await browser.newContext({ ...devices['iPad Mini'] });
  const page = await ctx.newPage();
  page.on('console', m => console.log('console:', m.text()));
  await page.goto(url);
  await page.waitForFunction(() => document.getElementById('status').textContent === '已连接', null, { timeout: 10000 });
  // Wait for a preview image from the PC.
  await page.waitForFunction(() => document.getElementById('preview').naturalWidth > 0, null, { timeout: 10000 });
  const box = await page.locator('#ink').boundingBox();
  const draw = async (pts) => {
    await page.mouse.move(box.x + pts[0][0] * box.width, box.y + pts[0][1] * box.height);
    await page.mouse.down();
    for (const p of pts.slice(1)) await page.mouse.move(box.x + p[0] * box.width, box.y + p[1] * box.height, { steps: 4 });
    await page.mouse.up();
  };
  // Stroke 1: zig-zag in red.
  await page.locator('#colors button').nth(1).click();
  await draw([[0.1, 0.2], [0.3, 0.5], [0.5, 0.2], [0.7, 0.5], [0.9, 0.2]]);
  // Stroke 2: highlighter.
  await page.locator('#tools button[data-tool="hl"]').click();
  await draw([[0.1, 0.7], [0.9, 0.7]]);
  // Stroke 3 then undo it.
  await page.locator('#tools button[data-tool="pen"]').click();
  await draw([[0.2, 0.85], [0.8, 0.85]]);
  await page.waitForTimeout(300);
  await page.locator('#undo').click();
  await page.waitForTimeout(800);
  await page.screenshot({ path: process.argv[3] });
  console.log('page label:', await page.locator('#page').textContent());
  await browser.close();
})().catch(e => { console.error(e); process.exit(1); });
