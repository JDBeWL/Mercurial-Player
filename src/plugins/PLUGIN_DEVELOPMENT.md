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
    "player:control",
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

## 可用权限

| 权限 | 说明 |
|------|------|
| `player:read` | 读取播放器状态（当前歌曲、播放进度等） |
| `player:control` | 控制播放器（播放、暂停、切歌等） |
| `library:read` | 读取音乐库和播放列表 |
| `lyrics:provider` | 提供歌词源 |
| `ui:extend` | 扩展用户界面 |
| `visualizer` | 注册可视化效果 |
| `theme` | 自定义主题 |
| `storage` | 本地数据存储 |
| `network` | 网络请求（仅 HTTPS） |

## API 参考

### 日志 (api.log)

```javascript
api.log.info('信息')
api.log.warn('警告')
api.log.error('错误')
api.log.debug('调试')
```

### 播放器 (api.player)

```javascript
// 获取播放状态
const state = await api.player.getState()
// state: { currentTrack, isPlaying, currentTime, duration, volume, repeatMode, isShuffle }

// 获取歌词
const lyrics = await api.player.getLyrics()

// 播放控制
await api.player.play()
await api.player.pause()
await api.player.togglePlay()
await api.player.next()
await api.player.previous()
await api.player.seek(30)  // 跳转到 30 秒
await api.player.setVolume(0.8)  // 设置音量 0-1
```

### 音乐库 (api.library)

```javascript
// 获取播放列表
const playlists = await api.library.getPlaylists()

// 获取当前播放列表
const current = await api.library.getCurrentPlaylist()

// 获取当前播放列表的歌曲
const tracks = await api.library.getTracks()
```

### 存储 (api.storage)

```javascript
// 存储数据
api.storage.set('myKey', { foo: 'bar' })

// 读取数据
const data = api.storage.get('myKey', defaultValue)

// 删除数据
api.storage.remove('myKey')

// 获取所有数据
const all = api.storage.getAll()
```

### 事件 (api.events)

```javascript
// 监听事件
api.events.on('plugin:activated', (data) => {
  api.log.info('插件被激活:', data)
})

// 取消监听
api.events.off('plugin:activated', callback)

// 触发自定义事件（供其他插件监听）
api.events.emit('myEvent', { data: 'hello' })
```

### UI 扩展 (api.ui)

```javascript
// 显示通知
api.ui.showNotification('操作成功', 'info')  // info, warning, error

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
```

### 歌词源 (api.lyrics)

```javascript
// 注册歌词源
api.lyrics.registerProvider({
  id: 'my-lyrics-source',
  name: '我的歌词源',
  
  // 搜索歌词
  async search(title, artist) {
    // 返回歌词列表
    return [
      { id: '1', title, artist, source: 'my-source' }
    ]
  },
  
  // 获取歌词内容
  async getLyrics(id) {
    // 返回 LRC 格式歌词
    return '[00:00.00]歌词内容...'
  }
})
```

### 可视化 (api.visualizer)

```javascript
// 注册可视化效果
api.visualizer.register({
  id: 'my-visualizer',
  name: '我的可视化',
  
  // 渲染函数，每帧调用
  render(canvas, audioData) {
    const ctx = canvas.getContext('2d')
    // 使用 audioData.waveform 或 audioData.spectrum 绘制
  }
})
```

### 网络 (api.network)

```javascript
// 发送 HTTPS 请求
const response = await api.network.fetch('https://api.example.com/data', {
  method: 'GET',
  headers: { 'Content-Type': 'application/json' }
})
const data = await response.json()
```

## 示例插件

### 播放计数器

```javascript
let currentTrackPath = null

const plugin = {
  activate() {
    this.intervalId = setInterval(() => this.checkPlayCount(), 1000)
  },

  deactivate() {
    clearInterval(this.intervalId)
  },

  async checkPlayCount() {
    const state = await api.player.getState()
    
    if (state.currentTrack && state.isPlaying) {
      const path = state.currentTrack.path
      
      if (path !== currentTrackPath) {
        currentTrackPath = path
        
        const counts = api.storage.get('playCounts', {})
        counts[path] = (counts[path] || 0) + 1
        api.storage.set('playCounts', counts)
        
        api.log.info(`播放次数: ${counts[path]}`)
      }
    }
  },

  getMostPlayed(limit = 10) {
    const counts = api.storage.get('playCounts', {})
    return Object.entries(counts)
      .sort((a, b) => b[1] - a[1])
      .slice(0, limit)
  }
}
```

## 安装插件

1. 打开设置 → 插件
2. 点击「打开插件目录」
3. 将插件文件夹复制到该目录
4. 点击「刷新」或重启应用

## 注意事项

- 插件代码运行在受限环境中，无法访问 `window`、`document` 等全局对象
- 网络请求仅支持 HTTPS
- 请勿在插件中存储敏感信息
- 插件应当优雅地处理错误，避免影响主应用
