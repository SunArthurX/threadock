import puppeteer from 'puppeteer-core';
const browser = await puppeteer.launch({
  executablePath: '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',
  headless: 'new',
  args: ['--no-sandbox', '--disable-setuid-sandbox'],
  defaultViewport: { width: 1440, height: 900 },
});
const page = await browser.newPage();
await page.evaluateOnNewDocument(() => {
  localStorage.setItem('ch-theme', 'light');
  localStorage.setItem('ch-text-size', 'md');
  localStorage.setItem('ch-onboarding-seen', '1');
  localStorage.setItem('ch-last-seen-version', '1.1.1');
});
await page.goto('http://localhost:1420/', { waitUntil: 'networkidle0' });
await new Promise(r => setTimeout(r, 1200));
await page.evaluate(() => {
  const btn = document.querySelector('button[title="设置"]');
  btn?.click();
});
await new Promise(r => setTimeout(r, 600));
// 滚到底部
await page.evaluate(() => {
  const body = document.querySelector('.settings-body');
  if (body) body.scrollTop = body.scrollHeight;
});
await new Promise(r => setTimeout(r, 300));
await page.screenshot({ path: 'docs/optimization-rounds/r13-settings-about-bottom.png', fullPage: false });
await page.close();
await browser.close();
