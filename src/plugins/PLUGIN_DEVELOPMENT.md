# Mercurial Player 插件开发指南

## 快速开始

### 插件结构

```
my-plugin/
├── manifest.json    # 插件清单（必需）
├── index.js         # 插件入口（必需）
└── README.md        # 插件说明（可选）
```

### manifest.json 示例

```json
{
  "id": "my-awesome-plugin",
  "name": "我的插件",
  "version": "1.0.0",
  "author": "Your Name",
  "description": "这是一个示例插件",
  "main": "index.js",
  "permissions": [
    "player:read",
    "storage"
  ],
  "autoActivate": true
}
```

### index.js 示例

```javascript
// 插件主入口
// api 对象由插件系统自动注入

const plugin = {
  // 插件激活时调用
  activate() {
    api.log.info('插件已激活！')
  },

  // 插件停用时调用
  deactivate() {
    api.log.info('插件已停用')
  },

  // 自定义方法
  async doSomething() {
    const state = await api.player.getState()
    api.log.info('当前播放:', state.currentTrack?.title)
  }
}
```

## 安全限制

插件运行在沙箱环境中，有以下限制：

### 禁止访问的全局对象

- `window`、`document`、`globalThis`、`self` - 浏览器全局对象
- `eval`、`Function` - 动态代码执行
- `fetch`、`XMLHttpRequest`、`WebSocket` - 网络请求（请使用 `api.network.fetch`）
- `localStorage`、`sessionStorage`、`indexedDB` - 存储（请使用 `api.storage`）
- `Proxy`、`Reflect` - 元编程
- `process`、`require`、`module` - Node.js 相关

### 安全的定时器

插件可以使用 `setTimeout` 和 `setInterval`，但有以下限制：
- `setTimeout` 最大延迟 60 秒
- `setInterval` 最小间隔 100ms

### 代码检查

插件代码在加载时会进行安全检查，以下模式会被拒绝：
- 访问 `__proto__`、`constructor`、`prototype` 进行原型链攻击
- 使用 `fromCharCode`、`fromCodePoint` 构造字符串绕过检测
- 动态 `import()` 语句
- 过多的动态属性访问（方括号语法）

## 可用权限

| 权限 | 说明 | 涉及的 API |
|------|------|-----------|
| `player:read` | 读取播放器状态 | `api.player.getState()`, `api.player.getLyrics()`, `api.player.getCurrentLyricIndex()` |
| `player:control` | 控制播放器 | `api.player.play()`, `api.player.pause()`, `api.player.next()`, `api.player.seek()`, `api.player.setVolume()` |
| `library:read` | 读取音乐库 | `api.library.getPlaylists()`, `api.library.getCurrentPlaylist()`, `api.library.getTracks()` |
| `lyrics:provider` | 提供歌词源 | `api.lyrics.registerProvider()`, `api.player.setLyrics()` |
| `ui:extend` | 扩展用户界面 | `api.ui.registerSettingsPanel()`, `api.ui.registerMenuItem()`, `api.ui.registerActionButton()`, `api.ui.registerPlayerDecorator()` |
| `visualizer` | 注册可视化效果 | `api.visualizer.register()` |
| `theme` | 自定义主题颜色 | `api.theme.setColors()` |
| `storage` | 本地数据存储 | `api.storage.*`, `api.file.*`, `api.clipboard.*` |
| `network` | 网络请求 | `api.network.fetch()` (仅 HTTPS) |

**注意：** 以下 API 不需要权限：
- `api.log.*` - 日志
- `api.utils.*` - 工具函数
- `api.theme.getCurrent()`, `api.theme.getCSSVariable()`, `api.theme.getAllColors()` - 读取主题
- `api.ui.showNotification()` - 显示通知
- `api.events.*` - 事件系统
- `api.commands.*` - 命令系统
- `api.shortcuts.*` - 快捷键

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
// 获取播放状态
const state = await api.player.getState()
// state: { currentTrack, isPlaying, currentTime, duration, volume, repeatMode, isShuffle }

// 获取歌词（已解析）
const lyrics = await api.player.getLyrics()
// lyrics: [{ time, texts: [原文, 翻译], karaoke, words }, ...]

// 获取当前歌词索引
const index = await api.player.getCurrentLyricIndex()
```

#### 播放控制 (需要 `player:control`)

```javascript
await api.player.play()
await api.player.pause()
await api.player.togglePlay()
await api.player.next()
await api.player.previous()
await api.player.seek(30)        // 跳转到 30 秒
await api.player.setVolume(0.8)  // 设置音量 0-1
```

#### 设置歌词 (需要 `lyrics:provider`)

```javascript
await api.player.setLyrics(parsedLyrics)
```

### 音乐库 (api.library)

需要 `library:read` 权限。

```javascript
// 获取所有播放列表
const playlists = await api.library.getPlaylists()

// 获取当前播放列表信息
const current = await api.library.getCurrentPlaylist()

// 获取当前播放列表的歌曲
const tracks = await api.library.getTracks()
```

### 存储 (api.storage)

需要 `storage` 权限。每个插件最多 1MB 存储空间。

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

无需权限。

```javascript
// 监听播放器事件
api.events.on('player:trackChanged', (data) => {
  // data: { track, isPlaying }
  api.log.info('歌曲切换:', data.track?.title)
})

api.events.on('player:stateChanged', (data) => {
  // data: { track, isPlaying }
  api.log.info('播放状态:', data.isPlaying ? '播放' : '暂停')
})

// 监听插件事件
api.events.on('plugin:activated', (data) => {
  api.log.info('插件被激活:', data.pluginId)
})

// 取消监听（停用时务必调用）
api.events.off('player:trackChanged')

// 触发自定义事件（供其他插件监听）
// 事件名会自动添加 plugin:{pluginId}: 前缀
api.events.emit('myEvent', { data: 'hello' })
```

### UI 扩展 (api.ui)

#### 通知 (无需权限)

```javascript
api.ui.showNotification('操作成功', 'info')    // info, warning, error
```

#### 扩展 (需要 `ui:extend`)

```javascript
// 注册操作按钮（显示在歌词区域）
api.ui.registerActionButton({
  id: 'my-button',
  name: '按钮提示文字',
  icon: 'favorite',  // Material Symbols 图标名
  location: 'lyrics',
  action: () => { /* 点击时执行 */ }
})

// 取消注册
api.ui.unregisterActionButton('my-button')

// 注册设置面板
api.ui.registerSettingsPanel({
  id: 'my-settings',
  name: '我的设置',
  component: MySettingsComponent  // Vue 组件
})

// 注册菜单项
api.ui.registerMenuItem({
  id: 'my-menu-item',
  name: '我的菜单',
  icon: 'star',
  action: () => { /* ... */ }
})

// 注册播放器装饰器
api.ui.registerPlayerDecorator({
  id: 'my-decorator',
  position: 'bottom',  // top, bottom, left, right
  component: MyDecoratorComponent
})
```

### 歌词源 (api.lyrics)

需要 `lyrics:provider` 权限。

```javascript
api.lyrics.registerProvider({
  id: 'my-lyrics-source',
  name: '我的歌词源',
  
  // 搜索歌词
  async search(title, artist) {
    return [
      { id: '1', title, artist, source: 'my-source' }
    ]
  },
  
  // 获取歌词内容
  async getLyrics(id) {
    return '[00:00.00]歌词内容...'  // LRC 格式
  }
})
```

### 可视化 (api.visualizer)

需要 `visualizer` 权限。

```javascript
api.visualizer.register({
  id: 'my-visualizer',
  name: '我的可视化',
  
  // 渲染函数，每帧调用
  render(canvas, audioData) {
    const ctx = canvas.getContext('2d')
    // audioData.waveform - 波形数据
    // audioData.spectrum - 频谱数据
  }
})
```

### 网络 (api.network)

需要 `network` 权限。仅支持 HTTPS。

```javascript
const response = await api.network.fetch('https://api.example.com/data', {
  method: 'GET',
  headers: { 'Content-Type': 'application/json' }
})
const data = await response.json()
```

### 主题 (api.theme)

#### 读取 (无需权限)

```javascript
// 获取当前主题信息
const theme = await api.theme.getCurrent()
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
api.utils.formatTime(125)   // '2:05'
api.utils.formatTime(3725)  // '1:02:05'

// 生成唯一 ID
const id = api.utils.generateId()  // 'pluginId-1703520000000-abc123xyz'
```

### 文件 (api.file)

需要 `storage` 权限。

```javascript
// 保存文件（弹出保存对话框）
const filePath = await api.file.saveAs(data, {
  defaultName: 'export.json',
  filters: [{ name: 'JSON 文件', extensions: ['json'] }],
  title: '导出数据'
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

需要 `storage` 权限。

```javascript
// 复制图片到剪贴板
await api.clipboard.writeImage(canvas)  // 支持 Canvas、Blob、DataURL

// 复制文本到剪贴板
await api.clipboard.writeText('Hello World')
```

### 快捷键 (api.shortcuts)

无需权限。

```javascript
// 注册快捷键
api.shortcuts.register({
  id: 'my-shortcut',
  name: '我的快捷键',
  key: 'Ctrl+Shift+M',  // 支持 Ctrl, Alt, Shift, Meta 组合
  description: '执行某个操作',
  action: () => {
    api.log.info('快捷键被触发!')
  }
})

// 取消注册（停用时务必调用）
api.shortcuts.unregister('my-shortcut')
```

### 命令 (api.commands)

无需权限。

```javascript
// 注册命令
api.commands.register({
  id: 'my-command',
  name: '我的命令',
  execute: async () => {
    api.log.info('命令执行')
  }
})

// 执行命令
await api.commands.execute('my-command')
```

## 完整示例

### 歌词截图分享插件

manifest.json:
```json
{
  "id": "lyrics-share",
  "name": "歌词截图分享",
  "version": "1.0.0",
  "author": "Your Name",
  "description": "生成歌词分享图片",
  "main": "index.js",
  "permissions": ["player:read", "storage", "ui:extend"],
  "autoActivate": true
}
```

index.js:
```javascript
const plugin = {
  activate() {
    api.log.info('歌词截图插件已激活')
    
    // 注册快捷键
    api.shortcuts.register({
      id: 'lyrics-share-copy',
      name: '复制歌词图片',
      key: 'Ctrl+Shift+C',
      action: () => this.copyImage()
    })
    
    // 注册操作按钮
    api.ui.registerActionButton({
      id: 'lyrics-share-btn',
      name: '复制歌词图片',
      icon: 'content_copy',
      location: 'lyrics',
      action: () => this.copyImage()
    })
  },

  deactivate() {
    api.shortcuts.unregister('lyrics-share-copy')
    api.ui.unregisterActionButton('lyrics-share-btn')
    api.log.info('歌词截图插件已停用')
  },

  async copyImage() {
    const state = await api.player.getState()
    if (!state.currentTrack) {
      api.ui.showNotification('没有正在播放的歌曲', 'warning')
      return
    }

    const lyrics = await api.player.getLyrics()
    const lyricIndex = await api.player.getCurrentLyricIndex()
    const theme = await api.theme.getCurrent()

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
      ctx.fillText(lyrics[lyricIndex].texts[0] || '', 400, 180)
      
      if (lyrics[lyricIndex].texts[1]) {
        ctx.globalAlpha = 0.7
        ctx.font = '24px system-ui'
        ctx.fillText(lyrics[lyricIndex].texts[1], 400, 220)
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
  }
}
```

### 播放统计插件

manifest.json:
```json
{
  "id": "play-stats",
  "name": "播放统计",
  "version": "1.0.0",
  "author": "Your Name",
  "description": "记录播放次数",
  "main": "index.js",
  "permissions": ["player:read", "storage"],
  "autoActivate": true
}
```

index.js:
```javascript
let lastTrackPath = null

const plugin = {
  activate() {
    api.log.info('播放统计插件已激活')
    
    // 监听歌曲切换事件
    api.events.on('player:trackChanged', (data) => {
      if (data.track && data.isPlaying) {
        this.recordPlay(data.track)
      }
    })
  },

  deactivate() {
    api.events.off('player:trackChanged')
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
      totalPlays: Object.values(counts).reduce((a, b) => a + b, 0)
    }
  }
}
```

## 安装插件

1. 打开设置 → 插件
2. 点击「打开插件目录」
3. 将插件文件夹复制到该目录
4. 点击「刷新」或重启应用

## 调试技巧

1. 使用 `api.log.debug()` 输出调试信息
2. 在开发者工具 (F12) 的控制台查看日志
3. 日志会带有 `[Plugin:pluginId]` 前缀

## 注意事项

1. **权限声明**：manifest.json 中必须声明所有需要的权限，否则 API 调用会抛出错误
2. **资源清理**：在 `deactivate()` 中取消所有事件监听、快捷键、定时器
3. **错误处理**：使用 try-catch 处理异步操作，避免影响主应用
4. **存储限制**：每个插件最多 1MB 存储空间，超出会自动清理旧数据
5. **网络安全**：仅支持 HTTPS 请求
6. **沙箱限制**：无法访问 DOM、window 等全局对象
