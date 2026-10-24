import { defineConfig } from 'vite'
import vue from '@vitejs/plugin-vue'
import { resolve } from 'path'

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
  worker: {
    format: 'es',
  },
  esbuild: {
    drop: process.env.TAURI_DEBUG ? [] : ['console', 'debugger'],
    legalComments: 'none',
  },
  build: {
    target:
      process.env.TAURI_PLATFORM == 'android'
        ? 'chrome110'
        : process.env.TAURI_PLATFORM == 'windows'
          ? 'chrome120'
          : 'safari17',
    minify: !process.env.TAURI_DEBUG ? 'esbuild' : false,
    sourcemap: !!process.env.TAURI_DEBUG,
    cssCodeSplit: true,
    cssMinify: true,
    chunkSizeWarningLimit: 600,
    emptyOutDir: true,
    reportCompressedSize: false,
    rollupOptions: {
      output: {
        manualChunks(id) {
          if (id.includes('node_modules/vue/') || id.includes('node_modules/@vue/')) {
            return 'vue-vendor'
          }
          if (id.includes('node_modules/pinia/')) {
            return 'pinia-vendor'
          }
          if (id.includes('node_modules/vue-i18n/') || id.includes('node_modules/@intlify/')) {
            return 'i18n-vendor'
          }
          if (id.includes('node_modules/@tauri-apps/')) {
            return 'tauri-vendor'
          }
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
      '@tauri-apps/api',
      '@tauri-apps/plugin-dialog',
      '@tauri-apps/plugin-fs',
      '@tauri-apps/plugin-store',
      '@tauri-apps/plugin-http',
      '@tauri-apps/plugin-global-shortcut',
      '@tauri-apps/plugin-process',
      '@tauri-apps/plugin-updater',
      '@tauri-apps/plugin-opener',
    ],
    exclude: ['@material/material-color-utilities'],
  },
})
