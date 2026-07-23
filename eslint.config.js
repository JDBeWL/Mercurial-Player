import js from '@eslint/js'
import tseslint from 'typescript-eslint'
import vue from 'eslint-plugin-vue'
import eslintConfigPrettier from 'eslint-config-prettier'

export default [
  // 全局忽略：构建产物、依赖、Rust 后端、覆盖率、用户插件
  {
    ignores: ['dist/**', 'node_modules/**', 'src-tauri/**', 'coverage/**', 'plugins/**'],
  },

  // JS 推荐规则
  js.configs.recommended,

  // TypeScript 推荐规则（非类型检查版本，无需 tsconfig project 引用，启动快）
  ...tseslint.configs.recommended,

  // Vue 推荐规则（flat config 格式）
  ...vue.configs['flat/recommended'],

  // Vue 文件：使用 TS parser 解析 <script lang="ts"> 块
  {
    files: ['**/*.vue'],
    languageOptions: {
      parserOptions: {
        parser: tseslint.parser,
      },
    },
  },

  // 测试文件：声明 vitest 全局变量（vitest.config.ts 中 globals: true）
  {
    files: ['tests/**/*.ts', 'src/**/*.{test,spec}.ts'],
    languageOptions: {
      globals: {
        describe: 'readonly',
        it: 'readonly',
        expect: 'readonly',
        beforeEach: 'readonly',
        afterEach: 'readonly',
        beforeAll: 'readonly',
        afterAll: 'readonly',
        vi: 'readonly',
      },
    },
  },

  // 自定义规则
  {
    rules: {
      // 禁用 no-undef：TypeScript 编译器已覆盖未定义变量检查，
      // 且 JS 版本的 no-undef 会对 Vue 模板中 <script setup> 的变量产生误报
      'no-undef': 'off',
      // Vue: 允许单词组件名（如 Settings、PlayerControls）
      'vue/multi-word-component-names': 'off',
      // TS: 允许使用 any（Tauri API 返回值等场景需要）
      '@typescript-eslint/no-explicit-any': 'off',
      // TS: 未使用变量仅警告，允许以 _ 前缀显式忽略
      '@typescript-eslint/no-unused-vars': [
        'warn',
        {
          argsIgnorePattern: '^_',
          varsIgnorePattern: '^_',
          caughtErrorsIgnorePattern: '^_',
        },
      ],
      // TS: ban-ts-comment 仅警告（@ts-ignore 在某些快速原型场景中仍有用）
      '@typescript-eslint/ban-ts-comment': 'warn',
      // TS: Function 类型仅警告
      '@typescript-eslint/no-unsafe-function-type': 'warn',
      // 允许 console（项目使用 logger，但开发调试时偶尔需要 console）
      'no-console': 'off',
      // ESLint 10 新规则，对现有代码库过于严格，暂时关闭
      'preserve-caught-error': 'off',
      // ESLint 10 新规则，暂时设为警告
      'no-useless-assignment': 'warn',
      // Vue: computed 属性中的副作用仅警告
      'vue/no-side-effects-in-computed-properties': 'warn',
    },
  },

  // 禁用与 Prettier 冲突的格式化规则（放在最后确保覆盖）
  eslintConfigPrettier,
]
