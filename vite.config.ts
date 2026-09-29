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
  // 插件沙箱使用 module Worker (blob URL 动态 import 仅在 ES Worker 中可用);
  // 产物为独立同源 chunk,CSP script-src 'self' 允许加载
  worker: {
    format: 'es',
  },
  // 生产构建时移除 console 和 debugger
  esbuild: {
    drop: process.env.TAURI_DEBUG ? [] : ['console', 'debugger'],
    legalComments: 'none',
  },
  build: {
    // CSS 产物里是否保留 -webkit- 前缀，完全由这个 target 决定（esbuild 按目标
    // 浏览器的支持情况增删前缀）。Android 上真正的运行时是 **System WebView 110**，
    // 比桌面端 WebView2 老得多：无前缀的 `mask-image` 要 Chrome 120 才支持，
    // 写成 safari17 会让压缩器把 `-webkit-mask-image` 整条删掉，
    // 于是靠 mask 做渐变的染色层退化成一块不透明纯色、把封面糊死。
    // 所以安卓必须显式按 chrome110 构建。
    target:
      process.env.TAURI_PLATFORM == 'android'
        ? 'chrome110'
        : process.env.TAURI_PLATFORM == 'windows'
          ? 'chrome120'
          : 'safari17',
    minify: !process.env.TAURI_DEBUG ? 'esbuild' : false,
    sourcemap: !!process.env.TAURI_DEBUG,
    // 每个 JS chunk 配一份独立 CSS，按需加载
    cssCodeSplit: true,
    cssMinify: true,
    chunkSizeWarningLimit: 600,
    // 每次构建前清空输出目录，避免旧产物残留
    emptyOutDir: true,
    // 构建日志不打印 gzip/brotli 体积（本地 Tauri 打包用不上，还拖慢构建）
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
          if (id.includes('node_modules/@fontsource')) {
            return 'fonts'
          }
        },
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
      // 假定模块无副作用，让未使用的导出能被彻底摇掉
      treeshake: {
        moduleSideEffects: false,
        propertyReadSideEffects: false,
      },
    },
  },
  resolve: {
    alias: {
      '@': resolve(import.meta.dirname, './src'),
    },
  },
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
