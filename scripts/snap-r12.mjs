import puppeteer from 'puppeteer-core';
import { mkdir } from 'node:fs/promises';
const OUT = 'docs/optimization-rounds';
await mkdir(OUT, { recursive: true });
const browser = await puppeteer.launch({
  executablePath: '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',
  headless: 'new',
  args: ['--no-sandbox', '--disable-setuid-sandbox', '--disable-gpu', '--hide-scrollbars'],
  defaultViewport: { width: 1440, height: 900, deviceScaleFactor: 1 },
});
const page = await browser.newPage();
await page.evaluateOnNewDocument(() => {
  try {
    localStorage.setItem('ch-theme', 'light');
    localStorage.setItem('ch-text-size', 'md');
    localStorage.setItem('ch-onboarding-seen', '1');
    localStorage.setItem('ch-last-seen-version', '1.1.1');
  } catch (e) {}
});
await page.goto('http://localhost:1420/', { waitUntil: 'networkidle0' });
await new Promise(r => setTimeout(r, 1200));
await page.screenshot({ path: `${OUT}/r12-base.png`, fullPage: false });

// ⌘, 唤起设置
await page.keyboard.down('Meta');
await page.keyboard.press(',');
await page.keyboard.up('Meta');
await new Promise(r => setTimeout(r, 600));
const settingsOpen = await page.evaluate(() => !!document.querySelector('.settings-backdrop'));
console.log('settings open after ⌘, :', settingsOpen);
await page.screenshot({ path: `${OUT}/r12-cmd-comma.png`, fullPage: false });

// Esc 关闭
await page.keyboard.press('Escape');
await new Promise(r => setTimeout(r, 400));
const closed = await page.evaluate(() => !document.querySelector('.settings-backdrop'));
console.log('settings closed after Esc:', closed);

await page.close();
await browser.close();
console.log('done.');
