// End-to-end check of the phone web client (see README "开发与测试").
// Usage: cargo run -p cb-ui --example remote_host -- url.txt 40 &  then  node scripts/remote-e2e.js url.txt out-dir
// Drives the phone web client in a mobile-emulated Chromium: strokes, undo,
// pinch zoom + writing while zoomed, and uploading three photos.
const { chromium, devices } = require('playwright');
const fs = require('fs');
const path = require('path');
(async () => {
  const url = fs.readFileSync(process.argv[2], 'utf8').trim();
  const out = process.argv[3] || '.';
  const browser = await chromium.launch();
  const ctx = await browser.newContext({ ...devices['iPad Mini'] });
  const page = await ctx.newPage();
  page.on('console', m => console.log('console:', m.text()));
  await page.goto(url);
  await page.waitForFunction(() => document.getElementById('status').textContent === '已连接', null, { timeout: 10000 });
  await page.waitForFunction(() => document.getElementById('preview').naturalWidth > 0, null, { timeout: 10000 });
  const box = await page.locator('#board').boundingBox();
  const draw = async (pts) => {
    await page.mouse.move(box.x + pts[0][0] * box.width, box.y + pts[0][1] * box.height);
    await page.mouse.down();
    for (const p of pts.slice(1)) await page.mouse.move(box.x + p[0] * box.width, box.y + p[1] * box.height, { steps: 4 });
    await page.mouse.up();
  };
  // Stroke 1: zig-zag in red. Stroke 2: highlighter. Stroke 3: undone.
  await page.locator('#colors button').nth(1).click();
  await draw([[0.1, 0.2], [0.3, 0.5], [0.5, 0.2], [0.7, 0.5], [0.9, 0.2]]);
  await page.locator('#tools button[data-tool="hl"]').click();
  await draw([[0.1, 0.7], [0.9, 0.7]]);
  await page.locator('#tools button[data-tool="pen"]').click();
  await draw([[0.2, 0.85], [0.8, 0.85]]);
  await page.waitForTimeout(300);
  await page.locator('#undo').click();
  await page.waitForTimeout(600);
  await page.screenshot({ path: path.join(out, 'phone_strokes.png') });

  // Pinch zoom 3x around the board centre with two touch points.
  const cdp = await ctx.newCDPSession(page);
  const cx = box.x + box.width / 2, cy = box.y + box.height / 2;
  const tp = (d) => [{ x: cx - d, y: cy, id: 1 }, { x: cx + d, y: cy, id: 2 }];
  await cdp.send('Input.dispatchTouchEvent', { type: 'touchStart', touchPoints: tp(40) });
  for (let i = 1; i <= 10; i++) await cdp.send('Input.dispatchTouchEvent', { type: 'touchMove', touchPoints: tp(40 + 8 * i) });
  await cdp.send('Input.dispatchTouchEvent', { type: 'touchEnd', touchPoints: [] });
  const zoom = await page.locator('#zoomText').textContent();
  console.log('zoom badge:', zoom);
  // Wait for the sharp detail frame for the zoomed region.
  await page.waitForFunction(() => document.getElementById('detail').style.display === 'block', null, { timeout: 10000 });
  // Write a short horizontal line in the middle of the zoomed view.
  await page.locator('#colors button').nth(4).click();
  await draw([[0.25, 0.5], [0.75, 0.5]]);
  await page.waitForTimeout(700);
  await page.screenshot({ path: path.join(out, 'phone_zoomed.png') });
  await page.locator('#zoomReset').click();
  await page.waitForTimeout(400);

  // Upload three photos through the album picker.
  const pngs = await page.evaluate(() => [['#e63946', 640, 480], ['#2a9d5a', 480, 640], ['#2463eb', 800, 500]].map(([c, w, h]) => {
    const cv = document.createElement('canvas'); cv.width = w; cv.height = h;
    const g = cv.getContext('2d'); g.fillStyle = c; g.fillRect(0, 0, w, h);
    g.fillStyle = '#fff'; g.font = 'bold 80px sans-serif'; g.fillText('照片', 40, h / 2);
    return cv.toDataURL('image/png').split(',')[1];
  }));
  await page.setInputFiles('#albumInput', pngs.map((b, i) => ({ name: `photo${i + 1}.png`, mimeType: 'image/png', buffer: Buffer.from(b, 'base64') })));
  await page.waitForFunction(() => document.getElementById('sendText').textContent.startsWith('已发送 3'), null, { timeout: 15000 });
  await page.waitForTimeout(1500);
  await page.screenshot({ path: path.join(out, 'phone_photos.png') });
  console.log('page label:', await page.locator('#page').textContent());
  await browser.close();
})().catch(e => { console.error(e); process.exit(1); });
