import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

// 开发模式：5173 热更新，/api 反代到本机 zimu-server（8080）
// 生产模式：npm run build 后由后端直接静态托管 dist（单服务单端口）
export default defineConfig({
  plugins: [react()],
  server: {
    // 显式绑 IPv4：vite 默认的 localhost 在本机解析为 ::1，127.0.0.1 会连不上
    host: "127.0.0.1",
    port: 5173,
    proxy: {
      "/api": { target: "http://127.0.0.1:8080", changeOrigin: true },
    },
  },
});
