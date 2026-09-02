# Mercurial Player 插件开发指南

## 快速开始

### 插件结构

**外部插件（JavaScript）：**

```
my-plugin/
├── manifest.json    # 插件清单（必需）
├── index.js         # 插件入口（必需）
└── README.md        # 插件说明（可选）
```

**内置插件（TypeScript）：**

```
src/plugins/builtins/
├── index.ts         # 导出所有内置插件
└── myPlugin.ts      # 插件实现文件
```

> **注意**：插件系统核心使用 TypeScript 编写，提供完整的类型定义和类型安全。外部插件使用 JavaScript 编写并在沙箱环境中运行，内置插件使用 TypeScript 编写并直接集成到应用中。

### manifest.json 示例

```json
{
  "id": "my-awesome-plugin",
  "name": "我的插件",
  "version": "1.0.0",
  "author": "Your Name",
  "description": "这是一个示例插件",
  "main": "index.js",
  "permissions": ["player:read", "storage"],
  "auto_activate": true
}
```

**字段说明：**

| 字段            | 必需 | 说明                                                                                       |
| --------------- | ---- | ------------------------------------------------------------------------------------------ |
| `id`            | 是   | 插件唯一标识，只能包含字母、数字、连字符和下划线                                           |
| `name`          | 是   | 插件显示名称                                                                               |
| `version`       | 否   | 版本号，须为 `x.y.z` 格式                                                                  |
| `author`        | 否   | 作者                                                                                       |
| `description`   | 否   | 描述                                                                                       |
| `main`          | 否   | 入口文件，默认 `index.js`                                                                  |
| `permissions`   | 否   | 权限列表，未声明则只有无需权限的 API 可用                                                  |
| `auto_activate` | 否   | 加载后是否自动激活，默认 `true`；设为 `false` 时需用户在设置页手动启用（注意是下划线命名） |

### 外部插件示例 (index.js)

```javascript
// 插件主入口:默认导出一个工厂函数,api 对象由插件系统自动注入

export default function (api) {
  return {
    // 插件激活时调用
    activate() {
      api.log.info('插件已激活！')
    },

    // 插件停用时调用
    deactivate() {
      api.log.info('插件已停用')
    },

    // 自定义方法
    doSomething() {
      const state = api.player.getState()
      api.log.info('当前播放:', state.currentTrack?.title)
    },
  }
}
```

### 内置插件示例 (TypeScript)

```typescript
import { PluginPermission, type PluginAPI, type BuiltinPluginDefinition } from '../pluginManager'

export const myPlugin: BuiltinPluginDefinition = {
  id: 'builtin-my-plugin',
  name: '我的插件',
  version: '1.0.0',
  author: 'Your Name',
  description: '插件描述',
  permissions: [PluginPermission.PLAYER_READ, PluginPermission.STORAGE],

  main: (api: PluginAPI) => {
    // 插件状态变量
    let isActive = false
    // 保存回调引用,deactivate 时需要用它来移除监听
    let trackChangedCallback: (data: unknown) => void

    return {
      async activate(): Promise<void> {
        api.log.info('插件已激活！')
        isActive = true

        // 注册事件监听器
        trackChangedCallback = (data) => {
          const { track } = data as { track: Track | null }
          api.log.info('歌曲切换:', track?.title)
        }
        api.events.on('player:trackChanged', trackChangedCallback)
      },

      deactivate(): void {
        api.log.info('插件已停用')
        isActive = false

        // 清理事件监听器
        // 注意: off 必须传入注册时的同一个回调引用,只传事件名无法移除监听
        api.events.off('player:trackChanged', trackChangedCallback)
      },

      // 自定义方法
      doSomething(): void {
        const state = api.player.getState()
        api.log.info('当前播放:', state.currentTrack?.title)
      },
    }
  },
}
```

## 安全限制

外部插件运行在 **Dedicated Worker 沙箱**中：插件代码在一个独立的 Worker 线程里执行，与主窗口物理隔离。Worker 中不存在 `window`、`document`、`localStorage`，也没有 Tauri IPC（`__TAURI_INTERNALS__`）——插件无法直接读写文件、调用系统命令或访问应用数据，只能通过 `api` 对象（由主窗口按 manifest 权限受控代理）访问应用功能。

### 沙箱内的真实边界

- 无法访问 DOM / `window` / `document` / `localStorage` / `indexedDB`
- 无法调用 Tauri IPC（`invoke`），即无法触达后端命令
- 全局 `postMessage` / `close` / `indexedDB` / `importScripts` / `addEventListener` 家族被移除（含原型链定义，`self.__proto__.postMessage` 等引用同样失效），无法伪造沙箱协议消息、窃听宿主下行消息或自行终止/外传
- 所有跨沙箱 API 调用由主窗口（可信侧）按精确路径白名单 + manifest 权限二次校验，Worker 内的预检仅用于快速失败
- 快捷键注册做全局冲突检测：同一按键组合已被占用时抛错拒绝（大小写与修饰键顺序归一化后比较）
- Worker 内的 `fetch` 受应用 CSP `connect-src` 白名单约束；跨域请求请声明 `network` 权限并使用 `api.network.fetch`
- UI 组件类扩展（`registerSettingsPanel` / `registerPlayerDecorator` / `visualizer.register`）无法跨沙箱渲染，沙箱插件调用会直接抛错——这类扩展请使用内置插件实现
- Canvas 相关 API（`api.utils.createCanvas` 等）基于 `OffscreenCanvas` / `ImageBitmap` 实现，接口与 DOM Canvas 兼容，但返回类型不同（无 `src` 等属性）
- 同步读 API（`player.getState()` / `storage.get()` / `library.*` / `theme.*`）从主窗口定期推送的镜像快照中读取，数据可能有数百毫秒的滞后

### 安全的定时器

插件可以使用 `setTimeout` 和 `setInterval`，但有以下限制：

- `setTimeout` 最大延迟 60 秒
- `setInterval` 最小间隔 100ms

插件停用时其 Worker 会被终止，所有未触发的定时器与事件监听随之一并释放。

### 代码检查

插件代码在加载时会进行安全检查，以下模式会被拒绝：

- 访问 `__proto__`、`constructor`、`prototype` 进行原型链攻击
- 使用 `fromCharCode`、`fromCodePoint` 构造字符串绕过检测
- 动态 `import()` 语句
- 过多的动态属性访问（方括号语法）

## 可用权限

| 权限              | 说明           | 涉及的 API                                                                                                                         |
| ----------------- | -------------- | ---------------------------------------------------------------------------------------------------------------------------------- |
| `player:read`     | 读取播放器状态 | `api.player.getState()`, `api.player.getLyrics()`, `api.player.getCurrentLyricIndex()`, `api.player.getCoverPath()`                |
| `player:control`  | 控制播放器     | `api.player.play()`, `api.player.pause()`, `api.player.next()`, `api.player.seek()`, `api.player.setVolume()`                      |
| `library:read`    | 读取音乐库     | `api.library.getPlaylists()`, `api.library.getCurrentPlaylist()`, `api.library.getTracks()`                                        |
| `lyrics:provider` | 提供歌词源     | `api.lyrics.registerProvider()`, `api.player.setLyrics()`                                                                          |
| `ui:extend`       | 扩展用户界面   | `api.ui.registerSettingsPanel()`, `api.ui.registerMenuItem()`, `api.ui.registerActionButton()`, `api.ui.registerPlayerDecorator()`, `api.commands.register()`, `api.shortcuts.register()` |
| `visualizer`      | 注册可视化效果 | `api.visualizer.register()`                                                                                                        |
| `theme`           | 修改主题颜色   | `api.theme.setColors()`                                                                                                            |
| `theme:read`      | 读取主题       | `api.theme.getCurrent()`, `api.theme.getCSSVariable()`, `api.theme.getAllColors()`(仅 `--md-sys-*` 变量)                          |
| `storage`         | 本地数据存储   | `api.storage.*`                                                                                                                    |
| `file:write`      | 写入文件       | `api.file.saveAs()`, `api.file.saveImage()`                                                                                        |
| `clipboard:write` | 写入剪贴板     | `api.clipboard.writeImage()`, `api.clipboard.writeText()`                                                                          |
| `network`         | 网络请求       | `api.network.fetch()` (仅 HTTPS)                                                                                                   |

**注意：** 以下 API 不需要权限：

- `api.log.*` - 日志
- `api.utils.*` - 工具函数
- `api.file.openScreenshotsDirectory()` - 打开截图目录
- `api.ui.showNotification()` - 显示通知
- `api.events.emit()` / `api.events.on()`（订阅 `plugin:*` 自定义与生命周期事件）- 事件系统
- `api.commands.execute()` - 仅能执行本插件注册的命令

事件订阅白名单：`player:trackChanged` / `player:stateChanged` 需要 `player:read` 权限（事件载荷含曲目本地路径等敏感数据）；`plugin:*` 事件无需权限；其他事件名会被拒绝。

## API 参考

### 日志 (api.log)

无需权限。

```javascript
api.log.info('信息')
api.log.warn('警告')
api.log.error('错误')
api.log.debug('调试')
```

### 播放器 (api.player)

#### 读取状态 (需要 `player:read`)

```javascript
// 获取播放状态（同步方法）
const state = api.player.getState()
// state: { currentTrack, isPlaying, currentTime, duration, volume, repeatMode, isShuffle }

// 获取歌词（异步方法，已解析）
const lyrics = await api.player.getLyrics()
// lyrics: [{ time, texts: [{ text, translation? }] }, ...]

// 获取当前歌词索引（同步方法）
const index = api.player.getCurrentLyricIndex()

// 获取当前曲目的封面缓存路径（异步方法，无封面时返回 null）
const coverPath = await api.player.getCoverPath()
```

#### 播放控制 (需要 `player:control`)

```javascript
api.player.play()
api.player.pause()
api.player.togglePlay()
await api.player.next()
await api.player.previous()
api.player.seek(30) // 跳转到 30 秒
api.player.setVolume(0.8) // 设置音量 0-1
```

#### 设置歌词 (需要 `lyrics:provider`)

```javascript
// 歌词格式: [{ time: number, texts: [{ text: string, translation?: string }] }, ...]
api.player.setLyrics(parsedLyrics)
```

### 音乐库 (api.library)

需要 `library:read` 权限。

```javascript
// 获取所有播放列表
const playlists = api.library.getPlaylists()
// playlists: [{ id, name, tracks }, ...]

// 获取当前播放列表信息
const current = api.library.getCurrentPlaylist()
// current: { id, name, tracks } | null

// 获取当前播放列表的歌曲
const tracks = api.library.getTracks()
// tracks: [{ path, title, artist, album, duration, ... }, ...]
```

### 存储 (api.storage)

需要 `storage` 权限。每个插件最多 1MB 存储空间。

写入是防抖持久化（300ms），插件停用和应用关闭时会立即保存；超出 1MB 时会自动裁剪过大的数组，localStorage 配额不足时会进一步紧急清理（数组只保留末尾 10 条），仍失败则放弃本次写入。

```javascript
// 存储数据（自动持久化）
api.storage.set('myKey', { foo: 'bar' })

// 读取数据
const data = api.storage.get('myKey', defaultValue)

// 删除数据
api.storage.remove('myKey')

// 获取所有数据
const all = api.storage.getAll()
```

### 事件 (api.events)

订阅白名单：`player:trackChanged` / `player:stateChanged` 需要 `player:read` 权限（事件载荷含曲目本地路径等敏感数据）；`plugin:*` 生命周期与自定义事件无需权限；其他事件名会被拒绝。`api.events.emit()` 无需权限（自动携带 `plugin:{pluginId}:` 前缀）。

```javascript
// 监听播放器事件
const onTrackChanged = (data) => {
  // data: { track, isPlaying }
  api.log.info('歌曲切换:', data.track?.title)
}
api.events.on('player:trackChanged', onTrackChanged)

api.events.on('player:stateChanged', (data) => {
  // data: { track, isPlaying }
  api.log.info('播放状态:', data.isPlaying ? '播放' : '暂停')
})

// 监听插件事件
api.events.on('plugin:activated', (data) => {
  api.log.info('插件被激活:', data.pluginId)
})

// 取消监听（停用时务必调用）
// 必须传入注册时的同一个回调引用,只传事件名无法移除监听
api.events.off('player:trackChanged', onTrackChanged)

// 触发自定义事件（供其他插件监听）
// 事件名会自动添加 plugin:{pluginId}: 前缀
api.events.emit('myEvent', { data: 'hello' })
```

### UI 扩展 (api.ui)

#### 通知 (无需权限)

```javascript
api.ui.showNotification('操作成功', 'info') // info, warning, error
```

#### 扩展 (需要 `ui:extend`)

```javascript
// 注册操作按钮（显示在歌词区域）
api.ui.registerActionButton({
  id: 'my-button',
  name: '按钮提示文字',
  icon: 'favorite', // Material Symbols 图标名
  location: 'lyrics',
  action: () => {
    /* 点击时执行 */
  },
})

// 取消注册
api.ui.unregisterActionButton('my-button')

// 注册菜单项
api.ui.registerMenuItem({
  id: 'my-menu-item',
  name: '我的菜单',
  icon: 'star',
  action: () => {
    /* ... */
  },
})
```

> **沙箱限制**：`api.ui.registerSettingsPanel` 与 `api.ui.registerPlayerDecorator`
> 需要跨沙箱传递 Vue 组件，仅内置插件可用；沙箱中的外部插件调用会抛出明确错误。
> 操作按钮 / 菜单项 / 通知对外部插件同样可用。

### 歌词源 (api.lyrics)

需要 `lyrics:provider` 权限。

```javascript
api.lyrics.registerProvider({
  id: 'my-lyrics-source',
  name: '我的歌词源',

  // 搜索歌词
  async search(title, artist) {
    return [{ id: '1', title, artist, source: 'my-source' }]
  },

  // 获取歌词内容
  async getLyrics(id) {
    return '[00:00.00]歌词内容...' // LRC 格式
  },
})
```

### 可视化 (api.visualizer)

需要 `visualizer` 权限。

> **沙箱限制**：`render` 回调需要主窗口的 Canvas 上下文，无法跨沙箱传递，
> 仅内置插件可用；沙箱中的外部插件调用 `api.visualizer.register` 会抛出明确错误。

```javascript
api.visualizer.register({
  id: 'my-visualizer',
  name: '我的可视化',

  // 渲染函数，每帧调用
  render(canvas, audioData) {
    const ctx = canvas.getContext('2d')
    // audioData.waveform - 波形数据
    // audioData.spectrum - 频谱数据
  },
})
```

### 网络 (api.network)

需要 `network` 权限。仅支持 HTTPS。

```javascript
const response = await api.network.fetch('https://api.example.com/data', {
  method: 'GET',
  headers: { 'Content-Type': 'application/json' },
})
const data = await response.json()
```

### 主题 (api.theme)

#### 读取 (无需权限)

```javascript
// 获取当前主题信息（同步方法）
const theme = api.theme.getCurrent()
// theme: { preference, isDark, primaryColor }

// 获取单个 CSS 变量
const color = api.theme.getCSSVariable('md-sys-color-primary')

// 获取所有主题颜色
const colors = api.theme.getAllColors()
// colors: { mdSysColorPrimary, mdSysColorOnPrimary, mdSysColorBackground, ... }
```

#### 设置 (需要 `theme` 权限)

```javascript
// 设置插件自定义颜色（会添加 --plugin-{pluginId}- 前缀）
await api.theme.setColors({ accent: '#ff0000' })
```

### 工具 (api.utils)

无需权限。

```javascript
// 创建 Canvas
const { canvas, ctx } = api.utils.createCanvas(800, 600)

// Canvas 转 Blob
const blob = await api.utils.canvasToBlob(canvas, 'image/png', 0.92)

// Canvas 转 DataURL (base64)
const dataURL = api.utils.canvasToDataURL(canvas, 'image/jpeg', 0.9)

// 加载图片
const img = await api.utils.loadImage('https://example.com/image.png')

// Blob 转 ArrayBuffer
const buffer = await api.utils.blobToArrayBuffer(blob)

// DataURL 转 Blob
const blob2 = api.utils.dataURLToBlob(dataURL)

// 格式化时间
api.utils.formatTime(125) // '2:05'
api.utils.formatTime(3725) // '1:02:05'

// 生成唯一 ID
const id = api.utils.generateId() // 'pluginId-1703520000000-abc123xyz'
```

### 文件 (api.file)

`saveAs` / `saveImage` 需要 `file:write` 权限;`openScreenshotsDirectory` 无需权限。

```javascript
// 保存文件（弹出保存对话框）
const filePath = await api.file.saveAs(data, {
  defaultName: 'export.json',
  filters: [{ name: 'JSON 文件', extensions: ['json'] }],
  title: '导出数据',
})
// data 可以是 Blob、Uint8Array 或 string

// 保存图片到 screenshots 目录
const imagePath = await api.file.saveImage(canvas, 'screenshot.png', 'png')
// 支持格式: 'png', 'jpeg', 'webp'
// canvas 可以是 HTMLCanvasElement、Blob 或 DataURL

// 打开截图目录
await api.file.openScreenshotsDirectory()
```

### 剪贴板 (api.clipboard)

需要 `clipboard:write` 权限。

```javascript
// 复制图片到剪贴板
await api.clipboard.writeImage(canvas) // 支持 Canvas、Blob、DataURL

// 复制文本到剪贴板
await api.clipboard.writeText('Hello World')
```

### 快捷键 (api.shortcuts)

注册与取消注册需要 `ui:extend` 权限（快捷键是全局键盘监听入口）。

```javascript
// 注册快捷键
api.shortcuts.register({
  id: 'my-shortcut',
  name: '我的快捷键',
  key: 'Ctrl+Shift+M', // 支持 Ctrl, Alt, Shift, Meta 组合
  description: '执行某个操作',
  action: () => {
    api.log.info('快捷键被触发!')
  },
})

// 取消注册（停用时务必调用）
api.shortcuts.unregister('my-shortcut')
```

### 命令 (api.commands)

注册需要 `ui:extend` 权限；执行仅限本插件注册的命令（跨插件执行等价于借道他人权限，会被静默拒绝）。

```javascript
// 注册命令
api.commands.register({
  id: 'my-command',
  name: '我的命令',
  execute: async () => {
    api.log.info('命令执行')
  },
})

// 执行命令
await api.commands.execute('my-command')
```

## 完整示例

### 示例 1: 歌词截图分享插件（外部插件 - JavaScript）

manifest.json:

```json
{
  "id": "lyrics-share",
  "name": "歌词截图分享",
  "version": "1.0.0",
  "author": "Your Name",
  "description": "生成歌词分享图片",
  "main": "index.js",
  "permissions": ["player:read", "storage", "ui:extend"]
}
```

index.js:

```javascript
export default function (api) {
  const plugin = {
    activate() {
      api.log.info('歌词截图插件已激活')

      // 注册快捷键
      api.shortcuts.register({
        id: 'lyrics-share-copy',
        name: '复制歌词图片',
        key: 'Ctrl+Shift+C',
        action: () => plugin.copyImage(),
      })

      // 注册操作按钮
      api.ui.registerActionButton({
        id: 'lyrics-share-btn',
        name: '复制歌词图片',
        icon: 'content_copy',
        location: 'lyrics',
        action: () => plugin.copyImage(),
      })
    },

    deactivate() {
      api.shortcuts.unregister('lyrics-share-copy')
      api.ui.unregisterActionButton('lyrics-share-btn')
      api.log.info('歌词截图插件已停用')
    },

    async copyImage() {
      const state = api.player.getState()
      if (!state.currentTrack) {
        api.ui.showNotification('没有正在播放的歌曲', 'warning')
        return
      }

      const lyrics = await api.player.getLyrics()
      const lyricIndex = api.player.getCurrentLyricIndex()
      const theme = api.theme.getCurrent()

      // 创建画布
      const { canvas, ctx } = api.utils.createCanvas(800, 400)

      // 绘制背景
      ctx.fillStyle = theme.isDark ? '#1a1a1a' : '#ffffff'
      ctx.fillRect(0, 0, 800, 400)

      // 绘制歌词
      ctx.fillStyle = api.theme.getCSSVariable('md-sys-color-primary') || '#6750a4'
      ctx.font = 'bold 32px system-ui'
      ctx.textAlign = 'center'

      if (lyrics && lyricIndex >= 0 && lyrics[lyricIndex]) {
        const line = lyrics[lyricIndex]
        ctx.fillText(line.texts[0]?.text || '', 400, 180)

        if (line.texts[0]?.translation) {
          ctx.globalAlpha = 0.7
          ctx.font = '24px system-ui'
          ctx.fillText(line.texts[0].translation, 400, 220)
          ctx.globalAlpha = 1
        }
      }

      // 绘制歌曲信息
      ctx.fillStyle = theme.isDark ? '#ffffff' : '#000000'
      ctx.font = 'bold 20px system-ui'
      ctx.fillText(state.currentTrack.title || '未知歌曲', 400, 320)

      ctx.font = '16px system-ui'
      ctx.globalAlpha = 0.7
      ctx.fillText(state.currentTrack.artist || '未知艺术家', 400, 350)

      // 复制到剪贴板
      await api.clipboard.writeImage(canvas)
      api.ui.showNotification('图片已复制到剪贴板', 'info')
    },
  }

  return plugin
}
```

### 示例 2: 播放统计插件（外部插件 - JavaScript）

manifest.json:

```json
{
  "id": "play-stats",
  "name": "播放统计",
  "version": "1.0.0",
  "author": "Your Name",
  "description": "记录播放次数",
  "main": "index.js",
  "permissions": ["player:read", "storage"]
}
```

index.js:

```javascript
export default function (api) {
  let lastTrackPath = null

  const plugin = {
    activate() {
      api.log.info('播放统计插件已激活')

      // 监听歌曲切换事件(保存回调引用,停用时移除监听需要用到)
      api.events.on('player:trackChanged', onTrackChanged)
    },

    deactivate() {
      // 必须传入注册时的同一个回调引用
      api.events.off('player:trackChanged', onTrackChanged)
      lastTrackPath = null
      api.log.info('播放统计插件已停用')
    },

    recordPlay(track) {
      if (!track.path || track.path === lastTrackPath) return

      lastTrackPath = track.path
      const counts = api.storage.get('playCounts', {})
      counts[track.path] = (counts[track.path] || 0) + 1
      api.storage.set('playCounts', counts)

      api.log.debug(`${track.title} 播放次数: ${counts[track.path]}`)
    },

    getMostPlayed(limit = 10) {
      const counts = api.storage.get('playCounts', {})
      return Object.entries(counts)
        .sort((a, b) => b[1] - a[1])
        .slice(0, limit)
        .map(([path, count]) => ({ path, count }))
    },

    getStats() {
      const counts = api.storage.get('playCounts', {})
      return {
        totalTracks: Object.keys(counts).length,
        totalPlays: Object.values(counts).reduce((a, b) => a + b, 0),
      }
    },
  }

  function onTrackChanged(data) {
    if (data.track && data.isPlaying) {
      plugin.recordPlay(data.track)
    }
  }

  return plugin
}
```

### 示例 3: 播放统计插件（内置插件 - TypeScript）

这是一个真实的内置插件示例，展示了如何使用 TypeScript 开发功能完整的插件。

```typescript
// src/plugins/builtins/playCount.ts
import {
  PluginPermission,
  type PluginAPI,
  type BuiltinPluginDefinition,
  type Track,
} from '../pluginManager'

interface PlayCountData {
  playCounts: Record<string, number>
  playHistory: HistoryEntry[]
  totalPlayTime: number
}

interface HistoryEntry {
  path: string
  title: string
  artist: string
  timestamp: number
}

export const playCountPlugin: BuiltinPluginDefinition = {
  id: 'builtin-play-count',
  name: '播放统计',
  version: '1.0.0',
  author: 'Mercurial Player',
  description: '记录每首歌曲的播放次数和播放历史',
  permissions: [PluginPermission.PLAYER_READ, PluginPermission.STORAGE],

  main: (api: PluginAPI) => {
    let lastTrackPath: string | null = null
    let playStartTime: number | null = null
    let hasRecordedCurrentTrack = false
    let pollingInterval: ReturnType<typeof setInterval> | null = null

    // 保存事件回调引用以便正确清理
    let trackChangedCallback: (data: unknown) => void
    let stateChangedCallback: (data: unknown) => void

    const loadData = (): PlayCountData => {
      return {
        playCounts: api.storage.get<Record<string, number>>('playCounts', {}),
        playHistory: api.storage.get<HistoryEntry[]>('playHistory', []),
        totalPlayTime: api.storage.get<number>('totalPlayTime', 0),
      }
    }

    const saveData = (data: PlayCountData): void => {
      api.storage.set('playCounts', data.playCounts)
      api.storage.set('playHistory', data.playHistory)
      api.storage.set('totalPlayTime', data.totalPlayTime)
    }

    const recordPlayCount = (track: Track): void => {
      if (!track || !track.path) return

      const data = loadData()
      data.playCounts[track.path] = (data.playCounts[track.path] || 0) + 1

      const historyEntry: HistoryEntry = {
        path: track.path,
        title: (track.title as string) || '',
        artist: (track.artist as string) || '',
        timestamp: Date.now(),
      }
      data.playHistory.unshift(historyEntry)
      if (data.playHistory.length > 100) {
        data.playHistory = data.playHistory.slice(0, 100)
      }

      saveData(data)
      api.log.debug(`播放记录: ${track.title} - 第 ${data.playCounts[track.path]} 次`)
    }

    const handleTrackChange = (newTrack: Track | null, isPlaying: boolean): void => {
      const newPath = newTrack?.path || null

      if (newPath !== lastTrackPath) {
        lastTrackPath = newPath
        hasRecordedCurrentTrack = false

        if (newTrack && isPlaying) {
          recordPlayCount(newTrack)
          hasRecordedCurrentTrack = true
          playStartTime = Date.now()
        }
      }
    }

    return {
      async activate(): Promise<void> {
        api.log.info('播放统计插件已激活')

        // 定义回调函数
        trackChangedCallback = (data) => {
          const { track, isPlaying } = data as { track: Track | null; isPlaying: boolean }
          handleTrackChange(track, isPlaying)
        }

        stateChangedCallback = (data) => {
          const { track, isPlaying } = data as { track: Track | null; isPlaying: boolean }
          handleTrackChange(track, isPlaying)
        }

        // 注册事件监听器
        api.events.on('player:trackChanged', trackChangedCallback)
        api.events.on('player:stateChanged', stateChangedCallback)

        // 轮询播放器状态
        pollingInterval = setInterval(async () => {
          try {
            const state = api.player.getState()
            handleTrackChange(state.currentTrack, state.isPlaying)
          } catch {
            // 忽略错误
          }
        }, 5000)
      },

      deactivate(): void {
        // 清理事件监听器
        if (trackChangedCallback) {
          api.events.off('player:trackChanged', trackChangedCallback)
        }
        if (stateChangedCallback) {
          api.events.off('player:stateChanged', stateChangedCallback)
        }

        // 清理定时器
        if (pollingInterval) {
          clearInterval(pollingInterval)
          pollingInterval = null
        }

        lastTrackPath = null
        playStartTime = null
        hasRecordedCurrentTrack = false
        api.log.info('播放统计插件已停用')
      },

      // 自定义方法：获取播放次数
      getPlayCount(trackPath: string): number {
        const data = loadData()
        return data.playCounts[trackPath] || 0
      },

      // 自定义方法：获取最常播放的歌曲
      getMostPlayed(limit = 10): { path: string; count: number }[] {
        const data = loadData()
        return Object.entries(data.playCounts)
          .sort((a, b) => b[1] - a[1])
          .slice(0, limit)
          .map(([path, count]) => ({ path, count }))
      },

      // 自定义方法：获取播放历史
      getPlayHistory(limit = 50): HistoryEntry[] {
        const data = loadData()
        return data.playHistory.slice(0, limit)
      },

      // 自定义方法：清除所有数据
      clearAllData(): void {
        api.storage.set('playCounts', {})
        api.storage.set('playHistory', [])
        api.storage.set('totalPlayTime', 0)
        api.log.info('播放统计数据已清除')
      },
    }
  },
}
```

**关键特性：**

- 使用 TypeScript 类型系统确保类型安全
- 正确管理事件监听器的生命周期
- 使用闭包保存插件状态
- 提供自定义方法供其他组件调用
- 使用 `api.storage` 持久化数据

## 插件系统架构

### 核心模块

1. **pluginManager.ts** - 插件管理器核心
   - 插件注册、激活、停用、卸载
   - 扩展点管理
   - 事件系统

2. **pluginTypes.ts** - 类型契约
   - 集中定义所有纯类型（PluginAPI、清单、实例、权限等）
   - 由 pluginManager.ts re-export 保持向后兼容

3. **pluginStorage.ts** - 存储持久化
   - 基于 localStorage 的插件数据持久化
   - 1MB 限额（超限裁剪大型数组）、QuotaExceeded 紧急清理
   - 防抖保存，插件停用/应用关闭时立即 flush

4. **pluginAPI.ts** - 插件 API 实现
   - 为插件提供安全的接口访问应用功能
   - 权限检查
   - API 方法实现

5. **pluginLoader.ts** - 插件加载器
   - 从文件系统加载外部插件
   - 验证插件清单
   - 代码安全检查

6. **sandbox/** - Worker 沙箱
   - `workerSandboxHost.ts` 主窗口侧宿主：RPC 分发、状态镜像推送、回调桥、Worker 生命周期
   - `workerCore.ts` Worker 侧运行时：插件 API 代理、参数/回调序列化
   - `workerBootstrap.ts` Worker 入口（消息接线）
   - `sandboxProtocol.ts` 两侧共享的消息协议
   - 外部插件代码在 Dedicated Worker 中执行，与主窗口权限物理隔离

7. **pluginSandbox.ts** - 内置插件执行环境
   - 为内置插件提供安全 console 代理与可清理的定时器
   - `validatePluginCode` 代码静态检查（外部插件进入 Worker 前的纵深防御层）

8. **builtins/** - 内置插件
   - TypeScript 编写的官方插件
   - 直接集成到应用中
   - 作为插件开发示例

### 插件类型对比

| 特性     | 外部插件 (JavaScript) | 内置插件 (TypeScript) |
| -------- | --------------------- | --------------------- |
| 开发语言 | JavaScript            | TypeScript            |
| 类型检查 | 无（可用 JSDoc）      | 完整类型检查          |
| 运行环境 | 沙箱隔离              | 直接运行              |
| 安全限制 | 严格限制              | 无限制                |
| 安装方式 | 用户安装              | 内置集成              |
| 更新方式 | 手动更新              | 应用更新              |
| 性能     | 略低                  | 最佳                  |
| 适用场景 | 第三方扩展            | 核心功能              |

## TypeScript 类型定义

插件系统使用 TypeScript 编写，提供完整的类型定义和类型安全。

### 核心类型

完整类型定义位于 `src/plugins/pluginTypes.ts`（`pluginManager.ts` 对其做了 re-export，因此从两个文件导入均可），主要类型包括：

```typescript
// 插件 API 接口
interface PluginAPI {
  pluginId: string
  permissions: readonly string[]
  log: LogAPI
  player: PlayerAPI
  library: LibraryAPI
  theme: ThemeAPI
  ui: UIAPI
  lyrics: LyricsAPI
  visualizer: VisualizerAPI
  commands: CommandsAPI
  shortcuts: ShortcutsAPI
  storage: StorageAPI
  events: EventsAPI
  network: NetworkAPI
  utils: UtilsAPI
  file: FileAPI
  clipboard: ClipboardAPI
}

// 播放器状态
interface PlayerState {
  currentTrack: Track | null
  isPlaying: boolean
  currentTime: number
  duration: number
  volume: number
  repeatMode: string
  isShuffle: boolean
}

// 歌曲信息
interface Track {
  path: string
  title?: string
  artist?: string
  album?: string
  duration?: number
  [key: string]: unknown
}

// 歌词行
interface LyricLine {
  time: number
  texts: { text: string; translation?: string }[]
  [key: string]: unknown
}

// 插件定义（内置插件）
interface BuiltinPluginDefinition {
  id: string
  name: string
  version?: string
  author?: string
  description?: string
  permissions?: PluginPermissionType[]
  main: (api: PluginAPI) => PluginInstance
}

// 插件实例
interface PluginInstance {
  activate?: () => void | Promise<void>
  deactivate?: () => void | Promise<void>
  [key: string]: unknown // 自定义方法
}

// 权限枚举
enum PluginPermission {
  PLAYER_READ = 'player:read',
  PLAYER_CONTROL = 'player:control',
  LIBRARY_READ = 'library:read',
  LYRICS_PROVIDER = 'lyrics:provider',
  UI_EXTEND = 'ui:extend',
  VISUALIZER = 'visualizer',
  THEME = 'theme',
  STORAGE = 'storage',
  NETWORK = 'network',
}
```

### 开发内置插件

内置插件使用 TypeScript 编写，享受完整的类型检查和 IDE 智能提示：

1. 在 `src/plugins/builtins/` 目录创建新的 `.ts` 文件
2. 导出一个 `BuiltinPluginDefinition` 对象
3. 在 `src/plugins/builtins/index.ts` 中导入并添加到 `builtinPlugins` 数组

**示例：**

```typescript
// src/plugins/builtins/myPlugin.ts
import {
  PluginPermission,
  type PluginAPI,
  type BuiltinPluginDefinition,
  type Track,
} from '../pluginManager'

export const myPlugin: BuiltinPluginDefinition = {
  id: 'builtin-my-plugin',
  name: '我的插件',
  version: '1.0.0',
  permissions: [PluginPermission.PLAYER_READ],

  main: (api: PluginAPI) => {
    return {
      activate(): void {
        api.log.info('插件已激活')
      },
      deactivate(): void {
        api.log.info('插件已停用')
      },
    }
  },
}
```

```typescript
// src/plugins/builtins/index.ts
import { myPlugin } from './myPlugin'
import type { BuiltinPluginDefinition } from '../pluginManager'

const builtinPlugins: BuiltinPluginDefinition[] = [
  myPlugin,
  // ... 其他插件
]

export default builtinPlugins
```

### 外部插件开发

外部插件使用 JavaScript 编写，但可以通过 JSDoc 注释获得类型提示：

```javascript
/**
 * @typedef {import('../pluginManager').PluginAPI} PluginAPI
 * @typedef {import('../pluginManager').Track} Track
 */

/**
 * @param {PluginAPI} api
 */
export default function (api) {
  return {
    activate() {
      api.log.info('插件已激活')
    },
    deactivate() {
      api.log.info('插件已停用')
    },
  }
}
```

## 安装插件

### 外部插件安装

1. 打开设置 → 插件
2. 点击「打开插件目录」
3. 将插件文件夹复制到该目录
4. 点击「刷新」或重启应用

### 内置插件开发

1. 在 `src/plugins/builtins/` 创建新的 `.ts` 文件
2. 导出 `BuiltinPluginDefinition` 对象
3. 在 `src/plugins/builtins/index.ts` 中导入并添加到数组
4. 重新编译应用

## 调试技巧

### 外部插件调试

1. 使用 `api.log.debug()` 输出调试信息
2. 在开发者工具 (F12) 的控制台查看日志
3. 日志会带有 `[Plugin:pluginId]` 前缀
4. 检查插件状态和错误信息

### 内置插件调试

1. 使用 `api.log.debug()` 输出调试信息
2. 使用 TypeScript 编译器检查类型错误
3. 使用 IDE 的断点调试功能
4. 查看浏览器开发者工具的控制台

## 最佳实践

### 通用最佳实践

1. **权限声明**：manifest.json 中必须声明所有需要的权限
2. **资源清理**：在 `deactivate()` 中取消所有事件监听、快捷键、定时器；`events.off` 必须传入注册时的同一个回调引用
3. **错误处理**：使用 try-catch 处理异步操作，避免影响主应用
4. **存储限制**：每个插件最多 1MB 存储空间，超出时自动裁剪过大的数组；建议只存储必要的数据
5. **网络安全**：仅支持 HTTPS 请求

### TypeScript 插件最佳实践

1. **类型安全**：充分利用 TypeScript 类型系统
2. **接口定义**：为插件数据定义清晰的接口
3. **类型导入**：从 `pluginManager.ts` 导入需要的类型
4. **类型断言**：在必要时使用类型断言，但要谨慎
5. **泛型使用**：在 `api.storage` 中使用泛型指定数据类型

### JavaScript 插件最佳实践

1. **JSDoc 注释**：使用 JSDoc 获得基本的类型提示
2. **沙箱限制**：无法访问 DOM、window 等全局对象
3. **代码检查**：避免使用被禁止的 API 和模式
4. **定时器限制**：`setTimeout` 最大 60 秒，`setInterval` 最小 100ms

## 常见问题

### Q: 内置插件和外部插件有什么区别？

**内置插件：**

- 使用 TypeScript 编写，享受完整的类型检查
- 直接集成到应用中，性能最佳
- 无沙箱限制，可以访问所有 API
- 需要重新编译应用才能更新
- 适合核心功能和官方扩展

**外部插件：**

- 使用 JavaScript 编写，在沙箱环境中运行
- 用户可以自行安装和卸载
- 有严格的安全限制
- 可以热更新，无需重启应用
- 适合第三方扩展和用户自定义功能

### Q: 如何在 JavaScript 插件中获得类型提示？

使用 JSDoc 注释：

```javascript
/**
 * @typedef {import('../pluginManager').PluginAPI} PluginAPI
 * @typedef {import('../pluginManager').Track} Track
 */

/**
 * @param {PluginAPI} api
 */
export default function (api) {
  return {
    activate() {
      // 现在可以获得 api 的类型提示
    },
  }
}
```

### Q: 插件可以访问哪些 API？

所有插件都可以访问 `api` 对象提供的接口，但需要在 manifest.json 中声明相应的权限。详见「可用权限」和「API 参考」章节。

### Q: 如何调试插件？

1. 使用 `api.log.debug()` 输出调试信息
2. 打开开发者工具 (F12) 查看控制台
3. 检查插件状态和错误信息
4. 对于内置插件，可以使用 IDE 的断点调试

### Q: 插件存储有大小限制吗？

是的，每个插件最多 1MB 存储空间。超出限制时会自动裁剪过大的数组（保留后半部分），localStorage 配额不足时会进一步紧急清理（数组只保留末尾 10 条）。写入是防抖持久化的（300ms），插件停用和应用关闭时会立即保存。建议只存储必要的数据。

### Q: 如何开发内置插件？

1. 在 `src/plugins/builtins/` 创建新的 `.ts` 文件
2. 参考 `playCount.ts` 的实现
3. 导出 `BuiltinPluginDefinition` 对象
4. 在 `index.ts` 中导入并添加到数组
5. 重新编译应用

### Q: 外部插件为什么不能访问 DOM？

出于安全考虑，外部插件运行在沙箱环境中，无法直接访问 DOM、window 等全局对象。如果需要操作 UI，请使用 `api.ui` 提供的接口。

### Q: 如何在插件之间通信？

使用事件系统：

```javascript
// 插件 A 发送事件
api.events.emit('myEvent', { data: 'hello' })

// 插件 B 监听事件
api.events.on('plugin:pluginA:myEvent', (data) => {
  api.log.info('收到消息:', data)
})
```

## 参考资源

- **类型契约定义**：`src/plugins/pluginTypes.ts`
- **API 实现**：`src/plugins/pluginAPI.ts`
- **插件管理器**：`src/plugins/pluginManager.ts`
- **存储持久化**：`src/plugins/pluginStorage.ts`
- **示例插件**：`src/plugins/builtins/playCount.ts`
- **插件加载器**：`src/plugins/pluginLoader.ts`
- **Worker 沙箱**：`src/plugins/sandbox/`（宿主 / Worker 运行时 / 入口 / 协议）
- **内置插件执行环境**：`src/plugins/pluginSandbox.ts`

## 贡献插件

如果你开发了有用的插件，欢迎分享给社区！

1. 确保插件遵循本文档的规范
2. 编写清晰的 README 说明
3. 测试插件的稳定性和安全性
4. 在项目仓库提交 Issue 或 Pull Request

---

**祝你开发愉快！**
