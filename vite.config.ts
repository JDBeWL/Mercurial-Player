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
      ignored: ['**/src-tauri/target/**'],
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
    chunkSizeWarningLimit: 600,
    // 每次构建前清空输出目录，避免旧产物残留
    emptyOutDir: true,
    // 启用 brotli 压缩预生成
    reportCompressedSize: false,
    rollupOptions: {
      output: {
        // 手动分包，优化缓存和加载
        manualChunks(id) {
          // Vue 核心（含 @vue/* 子包）独立分包，更新频率低
          if (id.includes('node_modules/vue/') || id.includes('node_modules/@vue/')) {
            return 'vue-vendor'
          }
          // 状态管理独立分包，与 Vue 版本更新频率不同
          if (id.includes('node_modules/pinia/')) {
            return 'pinia-vendor'
          }
          // i18n 独立分包
          if (id.includes('node_modules/vue-i18n/') || id.includes('node_modules/@intlify/')) {
            return 'i18n-vendor'
          }
          // 所有 Tauri 包 (API + 插件) 合并为单个 chunk
          // 插件内部依赖 @tauri-apps/api/core，拆分会导致跨 chunk 重复
          if (id.includes('node_modules/@tauri-apps/')) {
            return 'tauri-vendor'
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
      // 整包预构建 @tauri-apps/api，避免子模块分别预构建导致内部代码重复
      '@tauri-apps/api',
      // 预构建所有 Tauri 插件包，避免开发模式首次访问触发按需预构建导致页面重载
      '@tauri-apps/plugin-dialog',
      '@tauri-apps/plugin-fs',
      '@tauri-apps/plugin-store',
      '@tauri-apps/plugin-http',
      '@tauri-apps/plugin-global-shortcut',
      '@tauri-apps/plugin-process',
      '@tauri-apps/plugin-updater',
      '@tauri-apps/plugin-opener',
    ],
    exclude: [
      // 排除仅在特定场景使用的重型依赖，减少预构建时间
      '@material/material-color-utilities',
    ],
  },
})
