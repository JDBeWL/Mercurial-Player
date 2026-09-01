# Mercurial-Player 代码质量审查报告

**审查日期**：2026-08-31
**审查范围**：前端 97 个文件 / 25,448 行（TS + Vue）+ Rust 后端 53 个文件 / 15,986 行
**审查方式**：自动化工具全量扫描 + 人工逐条核实（报告中每个问题均已读源码确认，非工具转述）

---

## 一、总体结论

这个项目的**工程基础设施水平相当高**，自动化检查全线飘绿，代码规范度和资源清理纪律明显优于同类项目平均水平。

但在两个维度存在真实缺口：

1. **插件沙箱存在安全设计缺陷**（P0）—— 名为沙箱，实为约定
2. **错误处理链存在断点**（P1）—— 基础设施建好了，但关键调用点没有正确接线

其余问题集中在测试覆盖不均衡和少量重复代码，属于可渐进优化项。

---

## 二、自动化检查基线

| 检查项 | 结果 |
|---|---|
| ESLint（flat config） | ✅ 0 error 0 warning |
| vue-tsc --noEmit | ✅ 0 error |
| Prettier --check | ✅ 全部符合 |
| Vitest | ✅ 36 文件 / 719 测试全通过 |
| cargo clippy --all-targets | ✅ 0 warning |

### 测试覆盖率（v8）

| 模块 | 行覆盖 | 评价 |
|---|---|---|
| src/services | 90.9% | 良好 |
| src/utils | 89.32% | 良好 |
| src/stores | 47.03% | 偏弱 |
| src/plugins | 33.7% | 偏弱 |
| src/composables | 19.93% | **严重不足** |
| src/workers | 0% | **零覆盖** |
| **整体** | **45.12%** | — |

---

## 三、P0 — 必须修复

### 1. 插件沙箱形同虚设，等同主窗口权限的代码执行

**位置**：`src/plugins/moduleExecutor.ts:45-51`、`src/plugins/pluginSandbox.ts:179-220`、`src/plugins/pluginSandbox.ts:260-358`

**问题**：外置插件代码通过 `blob:` URL + 动态 `import()` 加载，运行在**主窗口真实全局作用域**中，而非隔离环境。

```ts
// moduleExecutor.ts:45-51
const blob = new Blob([moduleCode], { type: 'text/javascript' })
const url = URL.createObjectURL(blob)
mod = (await import(/* @vite-ignore */ url)) as { default?: unknown }
```

`createPluginSandbox()` 构造的 `allowedGlobals` 白名单，仅作为**第 2 个函数入参**传给插件工厂：

```ts
// pluginSandbox.ts:231-235
async execute<T>(fn) {
  return await (fn as (api, globals?) => ...)(api, this.globals)
}
```

插件完全可以忽略这个参数，直接访问 `window`、`document`、`fetch`、`localStorage`，以及最关键的 `window.__TAURI_INTERNALS__.invoke` —— 从而获得**完整 IPC 调用能力**（读写任意文件、执行系统命令取决于后端暴露的命令集）。

`validatePluginCode()` 是正则黑名单（先剥注释再剥字符串），可轻易绕过，例如字符串拼接访问、`Function('return this')()`、`Object.getOwnPropertyNames(globalThis)` 等。

**影响**：安装任意第三方插件 ≈ 授予其主窗口完整权限。由于 CSP 已放行 `blob:`（`tauri.conf.json`），防线缺失。

**建议**：
- 短期：改为 `<iframe sandbox="allow-scripts">` + 独立 origin 执行，通过 `postMessage` 通信（这是 WebView2 下唯一真正的隔离手段）
- 或：放弃执行不可信代码，改走 Tauri 后端 Rust 沙箱（如 wasm/wasmtime）执行插件逻辑
- 立刻：在插件安装 UI 增加显著安全警告，明确说明插件拥有完整应用权限

---

## 四、P1 — 应尽快修复

### 2. 配置保存失败被静默标记为成功

**位置**：`src/stores/config.ts:260-272`

```ts
this._savePromise = handlePromise(invoke('save_config', { config: configToSave }), {
  type: ErrorType.CONFIG_SAVE_ERROR,
  showToUser: false,
  throw: false,
})

await this._savePromise
this._savePromise = null

this._lastSavedConfig = configToSave   // ← 未检查 success，无条件执行
this._isDirty = false                  // ← 失败也标记为已保存
```

`handlePromise` 的返回值 `result.success` 从未被检查。磁盘满、权限错误、配置文件被占用时，配置实际上没写入，但前端认为已保存并清除了脏标记，后续不会再重试。

**影响**：用户设置静默丢失，且无任何提示。

**建议**：检查 `result.success`，失败时保留 `_isDirty = true` 以便重试，并提升告警级别。

---

### 3. `ErrorHandlerOptions.throw` 是死代码

**位置**：`src/utils/errorHandler.ts:155-198`（定义）、`:284-303`（`handlePromise`）、`src/types/index.ts:268`（类型声明）

`handle()` 方法解构的字段只有 6 个：

```ts
const {
  type, severity, context, silent, showToUser, userMessage,
} = options          // ← 没有 throw
```

`throw` 从未被读取。因此 `errorHandler.handle()` 永不抛出，`handlePromise()` 也永不 reject，永远返回 `{ success: false }`。

**影响**：类型定义承诺了一个不存在的行为。调用方写了 `throw: true` 却拿不到异常，会误以为错误已被上层处理。这是**最容易误导后续维护者的坑**。

**建议**：二选一 —— 要么在 `handle()` 中实现 `throw` 语义，要么从类型定义中移除该选项。

---

### 4. 全局无错误兜底通道

**位置**：`src/main.ts:120`、`src/main.ts:94`

```ts
const loadBuiltinPlugins = async (): Promise<void> => {
  await pluginManager.init()   // ← 未包 try/catch
  ...
}

loadBuiltinPlugins()           // ← 无 .catch
```

全项目搜索 `app.config.errorHandler`、`unhandledrejection`、`onErrorCaptured` **零命中**。

**影响**：启动阶段 `pluginManager.init()` 若失败，或任何未捕获的 Promise 拒绝，在**生产环境完全不可见** —— 注意 `vite.config.ts:19` 在生产构建时 `drop: ['console', 'debugger']`，连 console 输出都没有。用户遇到白屏或功能缺失时无法定位。

**建议**：
- `main.ts` 增加全局 `errorHandler` 与 `unhandledrejection` 监听
- 接入日志落盘（项目已有 `src/utils/logger.ts` 文件输出能力，接上即可）
- `loadBuiltinPlugins()` 补 `.catch`

---

### 5. 双歌词加载器，序号守卫各自为政

**位置**：`src/stores/player.ts:965`（守卫 `_activeLyricsRequestId`）、`src/composables/useLyrics.ts:90`（守卫 `loadSequence`）、`src/composables/useLyrics.ts:237`（无守卫）

存在两条并行的歌词加载路径，都写入同一份状态 `playerStore.lyrics`：

1. `playTrack` 成功后显式调用 `player.ts:541` → `store.loadLyrics(path, requestId)`
2. `useLyrics.ts:188` 的共享 watcher 监听 `currentTrack.path` 变化 → `loadLyrics(path)`（注意是 `immediate: true`）

两个守卫计数器**互相不失效**：

```ts
// player.ts:968
const lyricsRequestId = requestId ?? ++this._lyricsRequestId
this._activeLyricsRequestId = lyricsRequestId

// useLyrics.ts:93
const seq = ++loadSequence
```

更严重的是 `fetchAndSaveLyrics`（`useLyrics.ts:237`，由 `LyricsDisplay.vue:213` 手动触发）**完全没有序号守卫**，在线请求返回后无条件写入：

```ts
const onlineLrc = await fetchOnlineLyrics(track)
if (onlineLrc) {
  sharedLyrics.value = parsed
  playerStore.lyrics = parsed    // ← 无守卫，可能覆盖已切换的曲目
}
```

**影响**：快速切歌 + 在线歌词场景下，慢返回的旧请求会覆盖新曲目的歌词。

**建议**：收敛为单一加载路径，统一使用一个序号守卫；`fetchAndSaveLyrics` 必须补守卫生效。

---

## 五、P2 — 建议排期优化

### 6. 测试覆盖严重倾斜

`src/composables` 目录 18 个 composable，**仅 3 个**有测试（useGlobalKeyboard、useLyrics、useSliderFill）。完全无测试覆盖的有：

`useDominantColor`(385行)、`useImmersiveCover`(315行)、`useAppLifecycle`、`useDesktopLyrics`(443行)、`useLibrarySearch`、`useTrackInfo`、`useAutoUpdate`、`useImmersiveAutoHide`、`useDragValue`、`useVisualTime`、`useAlbumArtInteraction`、`useDeveloperMode`、`useWindowControls`、`useErrorNotification`、`useLyricsScroll`

另外：
- `src/plugins/pluginAPI.ts`（797 行，对外暴露给插件的 API 面）**无直接测试文件**
- `src/workers/coverUpscale.worker.ts` **零覆盖**
- `src/stores/player.ts`（1083 行）仅 2 个测试文件，覆盖 47%
- 未覆盖 store：playerCache、playerMediaCache、playerSession、shuffle、theme

**建议优先级**：先补 `pluginAPI.ts`（对外契约，回归风险最高）和 `useAppLifecycle`（关停流程，数据丢失风险高）。

---

### 7. `formatTime` 重复实现 4 份，且边界处理不一致

| 位置 | 实现 |
|---|---|
| `src/utils/format.ts:9` | 规范版，处理 `0 / NaN / Infinity / 负数` |
| `src/utils/fileUtils.ts:263` | 副本，仅判断 `isNaN / !isFinite` |
| `src/plugins/pluginAPI.ts:671` | 副本，同上 |
| `src/plugins/builtins/playCount.ts:30` | 副本 |
| `src/utils/lyricsParser.ts:487, 504` | 同文件内两份嵌套副本 |

差异示例：`format.ts` 对 `0` 和负数返回 `'0:00'`，而 `fileUtils.ts` 只拦截 NaN —— 负数会输出 `-1:-1:-1` 之类的错误结果。

**建议**：统一收敛到 `utils/format.ts`，`pluginAPI` 直接 re-export 给插件使用。

---

### 8. 定时器 / rAF 未清理

| 位置 | 问题 |
|---|---|
| `src/components/settings/PlayStatsSettings.vue:232` | `setTimeout(..., 100)` 未记录 ID，`onUnmounted` 只清了 interval 没清 timeout |
| `src/composables/useLyricsScroll.ts:143` | 内层 `setTimeout(..., 500)` 未记录，`dispose()` 只清理了外层 `scrollTimeout` |
| `src/components/LyricsDisplay.vue:386-387` | `requestAnimationFrame(forceSync)` 连锁调用，无取消机制 |

同时 `PlayStatsSettings.vue:149` 的 `refreshStats()` **没有 try/catch**，轮询时若插件抛错，每 5 秒产生一次 unhandled rejection。

---

### 9. 其他小问题

- **`src/plugins/pluginStorage.ts:33`** —— `JSON.parse(saved)` 结果未校验类型即赋给 `Record<string, unknown>`。若 localStorage 中存的是 `"null"` 或 `"5"`，`reactive()` 会拿到非普通对象，导致存储行为异常。
- **`src/utils/fileUtils.ts:255`** —— `formatFileSize` 传入负数时 `Math.log` 得 NaN，`sizes[NaN]` 为 `undefined`，输出 `"NaN undefined"`。虽然文件大小为负不太可能，但函数签名未约束。
- **`src/stores/player.ts:511`** —— `invoke('pause_track')` 是 fire-and-forget（`.catch` 只记日志），随后才 `await invoke('play_track')`。理论上 pause 若晚于 play 返回，新曲目会被立即暂停。实际概率低（后端命令通常按序处理），但值得改为串行 await。
- **`src/composables/useAppLifecycle.ts:58-64`** —— `beforeunload` 中 await 三个异步 IPC（`flushPendingSave` / `cleanup` / `pluginManager.cleanup`），WebView **不保证**等待其完成，关闭时有数据未落盘风险。
- **`src/utils/audioErrorClassifier.ts:29-61`** —— 仅匹配英文错误模式串，无中文模式（如「拒绝访问」「设备不可用」）。另外 `devicePatterns` 含 `'not initialized'` 过于宽泛，后端「播放器未初始化」会被误报为音频设备故障。
- **潜在未使用导出**：`colorContrast.ts` 的 `getContrastRatio` / `checkColorPairs` / `WCAG_STANDARDS`、`bundledFonts.ts` 的 `parseFontFileName`、`neteaseApi.ts` 的 `NeteaseAPI` —— 跨模块无引用，需确认是否该清理。

---

## 六、值得肯定的部分

审查中发现的**良好实践**，建议保持：

- **资源清理纪律优秀**：几乎所有 `addEventListener` 都有配对的 `removeEventListener`，定时器 ID 普遍被记录并在 `onUnmounted` / `dispose()` 中清理。这在 Vue 项目中并不常见。
- **tsconfig 严格度拉满**：`strict` + `noUncheckedIndexedAccess` + `noUnusedLocals` + `noUnusedParameters` + `noImplicitReturns`，同时还能保持零类型错误，说明类型定义质量高。
- **Rust 侧极其克制**：15,986 行代码仅 17 个 `unwrap()`、16 个 `expect()`、1 个 `panic!`，且经核实**绝大多数位于 `#[cfg(test)]` 测试代码内**。生产代码中唯一的 `unwrap`（`app_setup.rs:73`）也在 `#[cfg(debug_assertions)]` 块内。Clippy 零警告。
- **ESLint 配置注释详尽**：`eslint.config.js` 中每条规则的开关都写明了原因（如关闭 `no-undef` 是因为对 `<script setup>` 有误报），这是高质量的工程决策记录。
- **App.vue 不是 God Component**：虽然 1327 行，但拆解为 template 300 行 / script 320 行 / style 700 行，且逻辑已大量下沉到 composables。**无需拆分**。
- **依赖注入避免循环依赖**：`errorHandler.ts:18` 用 `setErrorHandlerTranslator()` 注入 i18n，避免工具模块反向依赖 app 装配层，设计干净。
- **性能细节考究**：`useLyrics.ts:174-180` 共享 watcher 避免三处组件重复触发加载与二分查找；`markRaw` 用于歌词避免无谓的深度响应式代理。

---

## 七、修复优先级建议

| 优先级 | 问题 | 预估影响 |
|---|---|---|
| 🔴 P0 | 插件沙箱隔离 | 安全漏洞，涉及第三方插件信任 |
| 🟠 P1 | 配置保存失败静默吞掉 | 用户数据丢失 |
| 🟠 P1 | `throw` 选项死代码 | 误导维护者，需清理或实现 |
| 🟠 P1 | 全局错误兜底缺失 | 生产问题不可诊断 |
| 🟠 P1 | 歌词加载守卫分裂 | 快速切歌时歌词错乱 |
| 🟡 P2 | 测试覆盖（尤其 composables / pluginAPI） | 长期回归风险 |
| 🟡 P2 | formatTime 重复实现 | 行为不一致隐患 |
| 🟡 P2 | 定时器 / rAF 未清理 | 轻微内存泄漏 |
| 🟡 P2 | 其余小问题 | 边界健壮性 |

---
