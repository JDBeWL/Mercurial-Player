import { defineConfig } from 'vite'
import vue from '@vitejs/plugin-vue'
import { resolve } from 'path'

// https://vitejs.dev/config/
export default defineConfig({
  plugins: [vue()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    watch: {
      ignored: ['**/src-tauri/target/**']
    },
  },
  envPrefix: ['VITE_', 'TAURI_'],
  // 生产构建时移除 console 和 debugger
  esbuild: {
    drop: process.env.TAURI_DEBUG ? [] : ['console', 'debugger'],
    legalComments: 'none',
  },
  build: {
    target: process.env.TAURI_PLATFORM == 'windows' ? 'chrome120' : 'safari17',
    minify: !process.env.TAURI_DEBUG ? 'esbuild' : false,
    sourcemap: !!process.env.TAURI_DEBUG,
    // 启用压缩和优化
    cssCodeSplit: true,
    cssMinify: true,
    chunkSizeWarningLimit: 800,
    // 启用 brotli 压缩预生成
    reportCompressedSize: false,
    rollupOptions: {
      output: {
        // 手动分包，优化缓存和加载
        manualChunks(id) {
          // Vue 核心生态
          if (id.includes('node_modules/vue/') || id.includes('node_modules/pinia/') || id.includes('node_modules/vue-i18n/')) {
            return 'vendor'
          }
          // Tauri 核心 API
          if (id.includes('node_modules/@tauri-apps/api/')) {
            return 'tauri-core'
          }
          // Tauri 插件 - 按功能分组
          if (id.includes('node_modules/@tauri-apps/plugin-dialog/') || id.includes('node_modules/@tauri-apps/plugin-fs/')) {
            return 'tauri-files'
          }
          if (id.includes('node_modules/@tauri-apps/plugin-global-shortcut/')) {
            return 'tauri-shortcuts'
          }
          if (id.includes('node_modules/@tauri-apps/plugin-store/')) {
            return 'tauri-store'
          }
          if (id.includes('node_modules/@tauri-apps/plugin-updater/') || id.includes('node_modules/@tauri-apps/plugin-process/')) {
            return 'tauri-update'
          }
          if (id.includes('node_modules/@tauri-apps/plugin-http/') || id.includes('node_modules/@tauri-apps/plugin-opener/')) {
            return 'tauri-network'
          }
          // Material Design 颜色工具库（体积较大）
          if (id.includes('node_modules/@material/material-color-utilities/')) {
            return 'material-colors'
          }
          // 字体资源
          if (id.includes('node_modules/@fontsource')) {
            return 'fonts'
          }
        },
        // 优化 chunk 文件名
        chunkFileNames: 'assets/[name]-[hash].js',
        entryFileNames: 'assets/[name]-[hash].js',
        assetFileNames: (assetInfo) => {
          const info = assetInfo.name || ''
          if (info.endsWith('.css')) {
            return 'assets/css/[name]-[hash][extname]'
          }
          if (info.endsWith('.woff2') || info.endsWith('.woff') || info.endsWith('.ttf')) {
            return 'assets/fonts/[name]-[hash][extname]'
          }
          return 'assets/[name]-[hash][extname]'
        },
      },
      // Tree-shaking 优化 - 更激进的配置
      treeshake: {
        moduleSideEffects: false,
        propertyReadSideEffects: false,
      },
    },
  },
  resolve: {
    alias: {
      '@': resolve(__dirname, './src'),
    },
  },
  // 优化依赖预构建
  optimizeDeps: {
    include: [
      'vue',
      'pinia',
      'vue-i18n',
      '@tauri-apps/api/core',
      '@tauri-apps/api/event',
    ],
    exclude: [
      // 排除仅在特定场景使用的重型依赖，减少预构建时间
      '@material/material-color-utilities',
    ],
  },
})
