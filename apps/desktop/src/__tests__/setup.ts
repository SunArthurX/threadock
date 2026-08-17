// vitest 全局 setup：注册 jest-dom 的 DOM 断言（toBeDisabled/toBeNull 等）
import "@testing-library/jest-dom/vitest";
// jsdom 未实现 scrollIntoView（真实浏览器原生支持）：知识面板滚动用
Element.prototype.scrollIntoView = () => {};
// jsdom 未实现 ResizeObserver：ScrollArea 组件用
// （仅实现组件用到的三个方法；构造签名差异用 unknown 中转断言补齐）
globalThis.ResizeObserver = class ResizeObserver {
  observe() {}
  unobserve() {}
  disconnect() {}
} as unknown as typeof ResizeObserver;
