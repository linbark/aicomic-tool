import { defineConfig } from 'vite'
import vue from '@vitejs/plugin-vue'

// https://vite.dev/config/
export default defineConfig({
  plugins: [vue()],
  clearScreen: false,
  server: {
    host: '0.0.0.0', // This is important for Tauri to be able to access the dev server
    port: 1420,
    strictPort: true,
    hmr: {
      protocol: 'ws',
      host: '127.0.0.1',
      port: 1420,
    },
  },
})
