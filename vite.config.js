import { defineConfig } from 'vite'
import vue from '@vitejs/plugin-vue'
import { resolve } from 'path'

export default defineConfig({
  plugins: [vue()],
  resolve: {
    alias: {
      '@renderer': resolve('src'),
      '@root': resolve('.'),
      '@': resolve('src')
    },
    extensions: ['.mjs', '.js', '.ts', '.jsx', '.tsx', '.json', '.vue']
  },
  clearScreen: false,
  build: {
    rollupOptions: {
      output: {
        // 拆分第三方依赖，避免单文件过大并提升缓存命中率
        manualChunks: {
          'element-plus': ['element-plus'],
          'vue-vendor': ['vue', 'vue-router', 'pinia', 'vue-i18n']
        }
      }
    }
  },
  server: {
    port: 1420,
    strictPort: true,
    watch: {
      ignored: ['**/src-tauri/**']
    }
  },
  envPrefix: ['VITE_', 'TAURI_']
})
