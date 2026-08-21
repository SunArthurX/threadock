#!/usr/bin/env node
/**
 * Round 10 全套截图：浅色/深色 + sm/md/lg/xl 字号，验证主题 + 字号 + focus
 */
import puppeteer from 'puppeteer-core';
import { mkdir } from 'node:fs/promises';

const OUT = 'docs/optimization-rounds';
const URL = 'http://localhost:1420/';
const CHROME = '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome';

await mkdir(OUT, { recursive: true });

const browser = await puppeteer.launch({
  executablePath: CHROME,
  headless: 'new',
  args: ['--no-sandbox', '--disable-setuid-sandbox', '--disable-gpu', '--hide-scrollbars'],
  defaultViewport: { width: 1440, height: 900, deviceScaleFactor: 1 },
});

// 检查 localStorage 的 keys
const page0 = await browser.newPage();
await page0.goto(URL, { waitUntil: 'networkidle0', timeout: 30000 });
const keys = await page0.evaluate(() => {
  const out = {};
  for (let i = 0; i < localStorage.length; i++) {
    const k = localStorage.key(i);
    out[k] = localStorage.getItem(k);
  }
  return out;
});
console.log('localStorage keys:', Object.keys(keys));
console.log('ch-theme:', keys['ch-theme']);
console.log('ch-text-size:', keys['ch-text-size']);
await page0.close();

// 4 个字号档 × 2 主题 = 8 张（settings 页验证字号控件）
// 8 个主页面截图（浅色 sm 主题）— 验证 round 10 polish 全部生效
const VARIANTS = [
  { theme: 'light', textSize: 'sm', suffix: 'light-sm' },
  { theme: 'light', textSize: 'md', suffix: 'light-md' },
  { theme: 'light', textSize: 'lg', suffix: 'light-lg' },
  { theme: 'light', textSize: 'xl', suffix: 'light-xl' },
  { theme: 'dark',  textSize: 'sm', suffix: 'dark-sm'  },
  { theme: 'dark',  textSize: 'lg', suffix: 'dark-lg'  },
];

for (const v of VARIANTS) {
  const page = await browser.newPage();
  await page.evaluateOnNewDocument(([theme, textSize]) => {
    try {
      localStorage.setItem('ch-theme', theme);
      localStorage.setItem('ch-text-size', textSize);
    } catch (e) {}
  }, [v.theme, v.textSize]);

  await page.goto(URL, { waitUntil: 'networkidle0', timeout: 30000 });
  await new Promise(r => setTimeout(r, 800));
  await page.screenshot({ path: `${OUT}/r10-${v.suffix}-overview.png`, fullPage: false });

  // 打开设置面板：找 aria-label/title 含"设置"的按钮
  const opened = await page.evaluate(() => {
    const btns = Array.from(document.querySelectorAll('button'));
    const btn = btns.find(b => {
      const t = (b.getAttribute('title') || '') + ' ' + (b.getAttribute('aria-label') || '');
      return /设置|Settings/i.test(t);
    });
    if (btn) { btn.click(); return true; }
    return false;
  });
  if (opened) {
    await new Promise(r => setTimeout(r, 500));
    await page.screenshot({ path: `${OUT}/r10-${v.suffix}-settings.png`, fullPage: false });
  } else {
    console.log(`  ! settings button not found for ${v.suffix}`);
  }
  await page.close();
  console.log(`✓ ${v.suffix}`);
}

await browser.close();
console.log('done.');
