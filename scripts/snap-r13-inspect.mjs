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
  // 给点 saved searches 数据好观察下拉
  localStorage.setItem('ch-search-history', JSON.stringify(['Claude Code 入门', 'threadock 主进程', 'provider:claude-code refactor']));
});
await page.goto('http://localhost:1420/', { waitUntil: 'networkidle0' });
await new Promise(r => setTimeout(r, 1000));
// 聚焦搜索框
await page.click('.search-box input');
await new Promise(r => setTimeout(r, 400));
// 截下拉
await page.screenshot({ path: 'docs/optimization-rounds/r13-search-dropdown.png', clip: { x: 60, y: 60, width: 800, height: 360 } });
// 移开看下保存按钮
await page.click('body');
await new Promise(r => setTimeout(r, 200));
await page.screenshot({ path: 'docs/optimization-rounds/r13-search-default.png', clip: { x: 60, y: 60, width: 800, height: 80 } });
// hover 搜索框
await page.hover('.search-box');
await new Promise(r => setTimeout(r, 200));
await page.screenshot({ path: 'docs/optimization-rounds/r13-search-hover.png', clip: { x: 60, y: 60, width: 800, height: 80 } });

// 检查 kb-copy 实际样式
const styles = await page.evaluate(() => {
  const btn = document.querySelector('.search-box .kb-copy');
  if (!btn) return null;
  const cs = getComputedStyle(btn);
  return {
    text: btn.textContent,
    opacity: cs.opacity,
    display: cs.display,
    visibility: cs.visibility,
    background: cs.backgroundColor,
    color: cs.color,
  };
});
console.log('kb-copy style:', JSON.stringify(styles, null, 2));

await page.close();
await browser.close();
