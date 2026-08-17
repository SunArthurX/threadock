// Vite ?raw 导入（读 Cargo.toml 版本用）
declare module "*?raw" {
  const content: string;
  export default content;
}
