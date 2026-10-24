import { defineConfig } from 'vitest/config'
import vue from '@vitejs/plugin-vue'
import { resolve } from 'path'

export default defineConfig({
  plugins: [vue()],
  test: {
    globals: true,
    environment: 'node',
    include: ['src/**/*.{test,spec}.{js,ts}', 'tests/**/*.{test,spec}.{js,ts}'],
    setupFiles: ['tests/setup.ts'],
    server: {
      deps: {
        inline: ['@material/material-color-utilities'],
      },
    },
    coverage: {
      provider: 'v8',
      include: ['src/**/*.ts', 'src/**/*.vue'],
      exclude: ['src/**/*.d.ts', 'src/types/**', 'src/**/*.test.ts', 'src/**/*.spec.ts'],
      clean: false,
      cleanAfterRun: false,
      thresholds: {
        lines: 86,
        branches: 76,
        functions: 80,
        statements: 84,
      },
    },
  },
  resolve: {
    alias: {
      '@': resolve(import.meta.dirname, './src'),
    },
  },
})
