#!/usr/bin/env node
/**
 * Round 11 截图：spring 模态框 + j/k 导航视觉验证
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

const page = await browser.newPage();
await page.evaluateOnNewDocument(() => {
  try {
    localStorage.setItem('ch-theme', 'light');
    localStorage.setItem('ch-text-size', 'md');
    // 跳过 onboarding + changelog
    localStorage.setItem('ch-onboarding-seen', '1');
    localStorage.setItem('ch-last-seen-version', '1.1.1');
  } catch (e) {}
});

await page.goto(URL, { waitUntil: 'networkidle0', timeout: 30000 });
await new Promise(r => setTimeout(r, 1000));

// 1. chat 页（j/k 导航的舞台）
await page.screenshot({ path: `${OUT}/r11-chat-list.png`, fullPage: false });

// 2. 模拟 j 键：找第一个 conv row，按 j 移焦点
const firstId = await page.evaluate(() => {
  const row = document.querySelector('[data-conv-row]');
  return row?.getAttribute('data-conv-row') ?? null;
});
console.log('first conv row:', firstId);

if (firstId) {
  // 按 j 两次
  await page.keyboard.press('j');
  await new Promise(r => setTimeout(r, 600));
  const after1 = await page.evaluate(() => {
    const active = document.querySelector('.list-item.active');
    return active?.getAttribute('data-conv-row') ?? null;
  });
  console.log('after j x1:', after1);

  await page.keyboard.press('j');
  await new Promise(r => setTimeout(r, 600));
  const after2 = await page.evaluate(() => {
    const active = document.querySelector('.list-item.active');
    return active?.getAttribute('data-conv-row') ?? null;
  });
  console.log('after j x2:', after2);

  await page.screenshot({ path: `${OUT}/r11-jk-after-2.png`, fullPage: false });

  // ⌘J 跳第一个
  await page.keyboard.down('Meta');
  await page.keyboard.press('j');
  await page.keyboard.up('Meta');
  await new Promise(r => setTimeout(r, 600));
  const afterCmdJ = await page.evaluate(() => {
    const active = document.querySelector('.list-item.active');
    return active?.getAttribute('data-conv-row') ?? null;
  });
  console.log('after ⌘J (first):', afterCmdJ);
  await page.screenshot({ path: `${OUT}/r11-cmdj-first.png`, fullPage: false });
}

// 3. 打开设置（验证 spring modal）
const settingsOpen = await page.evaluate(() => {
  const btn = document.querySelector('button[title="设置"]');
  if (btn) { btn.click(); return true; }
  return false;
});
if (settingsOpen) {
  await new Promise(r => setTimeout(r, 200));
  // 抓取动画半程
  await page.screenshot({ path: `${OUT}/r11-settings-spring-mid.png`, fullPage: false });
  await new Promise(r => setTimeout(r, 500));
  // 稳态
  await page.screenshot({ path: `${OUT}/r11-settings-spring-end.png`, fullPage: false });
  // 测 Esc 关闭
  await page.keyboard.press('Escape');
  await new Promise(r => setTimeout(r, 300));
  const closed = await page.evaluate(() => !document.querySelector('.settings-backdrop'));
  console.log('settings closed by Esc:', closed);
}

// 4. 打开命令面板（cmd-in 动画）
await page.keyboard.down('Meta');
await page.keyboard.press('k');
await page.keyboard.up('Meta');
await new Promise(r => setTimeout(r, 200));
await page.screenshot({ path: `${OUT}/r11-cmd-spring-mid.png`, fullPage: false });
await new Promise(r => setTimeout(r, 500));
await page.screenshot({ path: `${OUT}/r11-cmd-spring-end.png`, fullPage: false });
await page.keyboard.press('Escape');

await page.close();
await browser.close();
console.log('done.');
