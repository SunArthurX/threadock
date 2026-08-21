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
  localStorage.setItem('ch-view', 'chat');
  localStorage.setItem('ch-search-history', JSON.stringify(['Claude Code 入门', 'threadock 主进程', 'provider:claude-code refactor']));
});
await page.goto('http://localhost:1420/', { waitUntil: 'networkidle0' });
await new Promise(r => setTimeout(r, 1200));
// 顶栏基线（看 ImportMenu 顶钮改名 + 删除保存按钮）
await page.screenshot({ path: 'docs/optimization-rounds/r13-topbar.png', clip: { x: 60, y: 0, width: 1380, height: 80 } });
// 聚焦搜索 → 下拉 hover 样式
await page.click('.search-box input');
await new Promise(r => setTimeout(r, 400));
await page.hover('.search-history-item');
await new Promise(r => setTimeout(r, 200));
await page.screenshot({ path: 'docs/optimization-rounds/r13-search-dropdown.png', clip: { x: 60, y: 60, width: 800, height: 360 } });
// 点开 ImportMenu
await page.click('body');
await new Promise(r => setTimeout(r, 200));
await page.click('.import-trigger');
await new Promise(r => setTimeout(r, 400));
await page.screenshot({ path: 'docs/optimization-rounds/r13-import-menu.png', clip: { x: 900, y: 0, width: 540, height: 240 } });
// 关闭
await page.click('body');
await new Promise(r => setTimeout(r, 200));
// 打开设置 → 关于
await page.evaluate(() => {
  const btn = document.querySelector('button[title="设置"]');
  btn?.click();
});
await new Promise(r => setTimeout(r, 600));
await page.screenshot({ path: 'docs/optimization-rounds/r13-settings-about.png', fullPage: false });

await page.close();
await browser.close();
console.log('done.');
