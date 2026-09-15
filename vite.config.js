import { defineConfig } from "vite";

const host = process.env.TAURI_DEV_HOST;
const proxy = {
  "/api": "http://127.0.0.1:8888",
  "/ws": {
    target: "ws://127.0.0.1:8888",
    ws: true,
  },
};

export default defineConfig({
  root: "frontend",
  build: {
    outDir: "../src",
    emptyOutDir: true,
    // Android WebView 跟随系统，minSdk 是 24，老机器上版本可能很低
    // （实测 OnePlus 6 / Android 11 上是 92）。esbuild 默认按现代浏览器
    // 输出 CSS，会把 @media (max-width: 859px) 改写成范围语法
    // @media (width<=859px) —— 那是 Chrome 104 才支持的，老 WebView
    // 解析不了会整条丢掉，导致整个移动端样式块静默失效、退化成桌面布局。
    // 这里把 CSS 降级目标压到 104 以下，强制输出 max-width 写法。
    cssTarget: ["chrome87", "safari13", "firefox78", "edge88"],
  },
  server: {
    host: host || "127.0.0.1",
    port: 1420,
    strictPort: true,
    proxy,
  },
  preview: {
    host: "127.0.0.1",
    port: 4173,
    strictPort: true,
    proxy,
  },
});
