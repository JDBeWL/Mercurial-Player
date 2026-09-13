import { defineConfig } from 'vitest/config'
import vue from '@vitejs/plugin-vue'
import { resolve } from 'path'

export default defineConfig({
  plugins: [vue()],
  test: {
    globals: true,
    // 默认 node 环境(启动快);需要 DOM 的测试文件在文件顶部用
    // @vitest-environment happy-dom 注释单独覆盖
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
      // 仅统计 .ts;.vue 组件未纳入统计(加进来会让整体覆盖率明显下降)
      include: ['src/**/*.ts'],
      exclude: ['src/**/*.d.ts', 'src/types/**', 'src/**/*.test.ts', 'src/**/*.spec.ts'],
      // 关闭运行前后的目录清理:清理会一次性删除 coverage/ 下大量报告文件,
      // 在带批量删除防护的环境中会被拦截并以 Unhandled Error 中断;
      // 报告文件名为固定值,重复运行时直接覆盖,不会无限累积
      clean: false,
      cleanAfterRun: false,
      thresholds: {
        lines: 83,
        branches: 77,
        functions: 79,
        statements: 82,
      },
    },
  },
  resolve: {
    alias: {
      '@': resolve(import.meta.dirname, './src'),
    },
  },
})
