import { defineConfig } from 'vitest/config'
import vue from '@vitejs/plugin-vue'
import { resolve } from 'path'

export default defineConfig({
  plugins: [vue()],
  test: {
    globals: true,
    // 默认 node 环境(快速启动);需要 DOM 的测试文件用
    // @vitest-environment happy-dom 控制注释覆盖
    environment: 'node',
    include: ['src/**/*.{test,spec}.{js,ts}', 'tests/**/*.{test,spec}.{js,ts}'],
    setupFiles: ['tests/setup.ts'],
    // @material/material-color-utilities@0.4.0 内部 import 省略了 .js 扩展名,
    // Node ESM 严格解析会失败;交给 vite 处理会自动补全扩展名
    server: {
      deps: {
        inline: ['@material/material-color-utilities'],
      },
    },
    coverage: {
      provider: 'v8',
      include: ['src/**/*.ts'],
      exclude: ['src/**/*.d.ts', 'src/types/**', 'src/**/*.test.ts', 'src/**/*.spec.ts'],
      // 初始阈值设低,逐步提升
      thresholds: {
        lines: 40,
        branches: 30,
        functions: 30,
        statements: 40,
      },
    },
  },
  resolve: {
    alias: {
      '@': resolve(import.meta.dirname, './src'),
    },
  },
})
