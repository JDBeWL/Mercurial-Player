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

  // TypeScript 推荐规则（非类型检查版本）
  // 未使用 recommendedTypeChecked 因为：
  // 1. 现有代码库有大量 any 使用，启用后产生 400+ 错误
  // 2. ESLint 10 + typescript-eslint 8 的 parserServices 对配置文件有兼容性问题
  // 后续可逐步修复类型安全问题后再升级到 recommendedTypeChecked
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

  // 测试文件：声明完整的 vitest 全局变量（vitest.config.ts 中 globals: true）
  // 包含 vi/vitest 等 API，避免手动声明遗漏
  {
    files: ['tests/**/*.ts', 'src/**/*.{test,spec}.ts'],
    languageOptions: {
      globals: {
        describe: 'readonly',
        it: 'readonly',
        test: 'readonly',
        expect: 'readonly',
        beforeEach: 'readonly',
        afterEach: 'readonly',
        beforeAll: 'readonly',
        afterAll: 'readonly',
        vi: 'readonly',
        vitest: 'readonly',
      },
    },
    rules: {
      // 允许测试中使用 any（mock 场景常见）
      '@typescript-eslint/no-explicit-any': 'off',
      // 测试文件中允许未使用的变量（describe/it 回调参数等）
      '@typescript-eslint/no-unused-vars': 'off',
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
      // TS: any 禁止（当前代码库已无 any;Tauri API 返回值应显式声明类型）,
      // 测试文件中完全允许 (见上方 overrides)
      '@typescript-eslint/no-explicit-any': 'error',
      // TS: 未使用变量禁止,允许以 _ 前缀显式忽略
      '@typescript-eslint/no-unused-vars': [
        'error',
        {
          argsIgnorePattern: '^_',
          varsIgnorePattern: '^_',
          caughtErrorsIgnorePattern: '^_',
        },
      ],
      // TS: 禁止 @ts-ignore（应使用 @ts-expect-error 带原因），阻断 CI
      '@typescript-eslint/ban-ts-comment': 'error',
      // TS: Function 类型禁止 (当前代码库已无违规)
      '@typescript-eslint/no-unsafe-function-type': 'error',
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
