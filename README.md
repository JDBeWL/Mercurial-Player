<h1 align="center" style="font-size: 24px; font-weight: bold;">
   相反している因果と応報<br/>
   截然颠倒的因果与报应<br/>
   誠実さんはくたびれ儲け<br/>
   诚实只能换来一场空<br/>
   愛を歌えばなんとかなると思っていた<br/>
   好似只要高歌爱 一切都能顺利解决<br/>
   こんな時代じゃ本質も見えないが...<br/>
   这个时代连本质都模糊不清了…
</h1>

![你会做梦吗](/君は夢を見ますか.png)

<h3 align="center">
   是基于Tauri开发的音乐播放器。
</h3>

<p align="center">
  <a href="https://skillicons.dev">
    <img src="https://skillicons.dev/icons?i=vite,vue,rust,tauri,typescript" />
  </a>
</p>

# 功能特性

## 音频播放

- [x] 支持格式：Symphonia支持什么就支持什么
- [x] 支持的播放方式：共享模式下面使用rodio(rodio已内部使用Symphonia解码)，在Windows平台上特殊支持WASAPI独占模式访问
- [x] 切换设备：支持在断开，手动切换下自动切换输出设备，WASAPI独占模式也能通过监听事件实现在共享模式和独占模式的切换，不需要切换下一首，无论是在播放中还是没有播放情况下这个功能基本可用。
- [x] 高采样率支持：Rubato能提供什么样的重采样就大概有什么采样
- [x] EQ均衡器：这个功能到底怎么写更好，总是炸
- [x] 淡入淡出：切歌时平滑过渡（独占模式50ms淡出），pause/resume消除爆音（30ms淡入淡出）
- [x] WASAPI独占模式音频加速解码：Windows下的WASAPI独占模式下特殊支持SIMD处理部分数据，如果不支持会fallback到SSE2加速。在不支持软件模拟的ARM64环境下，或不支持SSE2的64位的桌面X86处理器平台在支持WASAPI且能驱动这个WebView2的Windows环境中（~~按道理任何x86_64的CPU都应该支持这个SSE2吧，如果有当我什么都没说~~），这种情况将fallback到不加速

## 歌词功能

- [x] 多格式支持：LRC、ASS
- [x] 自动加载：根据音频文件名自动查找匹配的歌词文件
- [x] 在线歌词：从网易云音乐Web API获取歌词
- [x] 歌词样式：支持传统的播放器的滚动歌词和一种更加消耗资源的滚动歌词显示方式
- [x] 歌词对齐：左/中/右
- [x] 歌词偏移
- [x] 点击歌词跳转
- [x] 卡拉OK逐字高亮（ASS格式）
- [x] 双语歌词显示
- [x] 歌词字体可选：内置霞鹜文楷屏幕版（LXGW WenKai Screen）与程序默认的Noto Sans，支持软件同级 `fonts/` 目录动态加载与构建期打包（`src/assets/fonts/lyrics/`）自定义字体，也可选系统字体
- [x] 桌面歌词

## 播放控制

- [x] 播放/暂停/上一首/下一首
- [x] 进度条拖动跳转
- [x] 音量调节
- [x] 单曲循环/列表循环
- [x] 随机播放（Knuth Shuffle‌）

## 可视化

- [x] 实时FFT频谱
- [x] 动画帧率理论无上限
- [x] 垂直同步支持
- [x] 歌词/可视化视图切换
- [x] 沉浸式封面，使用pica库尝试解决封面在缩放后糊的问题

## 播放列表

- [x] 文件夹扫描
- [x] 子目录扫描
- [x] 元数据读取（单次采样精度、比特率、封面、标题、艺术家）
- [x] 按文件夹创建播放列表
- [x] 批量元数据获取优化
- [x] 全文搜索：基于Tantivy索引歌曲、艺术家、专辑

## 界面

- [x] 浅色/深色主题
- [x] 主题颜色系统
- [x] Mini模式
- [x] 中文/English

## 配置

- [x] 配置持久化
- [x] 标题提取配置

## 插件

- [x] 插件系统
- [x] 播放统计（内置插件）

## 安全

- [x] 文件系统安全限制

## 其他

- [x] 可以在任务栏控制播放（但是必须要先有播放列表）
- [x] 有缓存防止反复提取封面到内存，尽量减小大量歌曲在文件夹中对主控的读开销
- [x] 更好的字体显示（@fontsource/roboto）
- [x] 应用内自动更新（Tauri Updater）

# 技术栈

## 前端

| 技术                               | 版本     |
| ---------------------------------- | -------- |
| Vue                                | ^3.5.34  |
| Vite                               | ^8.2.1   |
| Pinia                              | ^4.0.2   |
| Vue I18n                           | ^11.4.7  |
| Sass                               | ^1.99.0  |
| TypeScript                         | ^6.0.3   |
| vue-tsc                            | ^3.3.7   |
| Tauri API                          | ^2.11.1  |
| Tauri 官方插件                     | ^2.x     |
| @vitejs/plugin-vue                 | ^6.0.7   |
| Vitest                             | ^4.1.6   |
| @vue/test-utils                    | ^2.4.10  |
| Happy DOM                          | ^20.11.6 |
| ESLint                             | ^10.7.0  |
| Prettier                           | ^3.9.6   |
| esbuild                            | ^0.28.2  |
| @material/material-color-utilities | ^0.4.0   |
| pica                               | ^10.0.3  |
| @fontsource/roboto                 | ^5.3.0   |
| lxgw-wenkai-screen-webfont         | ^1.7.0   |

## 后端 (Rust)

| 技术                 | 版本  | 说明                                                            |
| -------------------- | ----- | --------------------------------------------------------------- |
| Rust                 | 1.92+ |                                                                 |
| Tauri                | 2.11  |                                                                 |
| Symphonia            | 0.6   | 音频解码器                                                      |
| Rodio                | 0.22  | 音频播放引擎                                                    |
| CPAL                 | 0.17  |                                                                 |
| AudioAdapter         | 4.0   | 音频缓冲区抽象                                                  |
| WASAPI               | 0.23  | Windows独占模式音频                                             |
| Windows API          | 0.62  | Win32 API绑定                                                   |
| Winreg               | 0.56  | 注册表访问（读取系统字体列表）                                  |
| Rubato               | 4.0   | 音频重采样                                                      |
| Lofty                | 0.24  | 音频元数据读取                                                  |
| Tantivy              | 0.26  | 全文搜索引擎                                                    |
| Rayon                | 1.x   | 并行数据处理                                                    |
| Spectrum Analyzer    | 1.7   | 频谱分析                                                        |
| Walkdir              | 2.x   | 目录遍历                                                        |
| Dirs                 | 6.x   | 系统目录定位                                                    |
| Serde                | 1.x   | 序列化/反序列化                                                 |
| Tokio                | 1.x   |                                                                 |
| Crossbeam-channel    | 0.5   | 跨线程通信                                                      |
| Display Info         | 0.5   | 显示器信息（获取刷新率）                                        |
| Unicode Segmentation | 1.x   | 文本分段（桌面歌词逐字渲染）                                    |
| Tauri 插件           | 2.x   | log/dialog/fs/http/opener/process/store/updater/global-shortcut |
| Criterion            | 0.5   | 基准测试                                                        |

# 部署

## 环境要求

1. **Node.js** - 推荐v24+
2. **Rust** - 需要1.92或更高版本
   - Windows: 访问 [rustup.rs](https://rustup.rs/) 下载安装
   - 安装后运行 `rustup update` 确保版本最新
3. **Tauri 依赖** - 参考 [Tauri 官方文档](https://tauri.app/start/prerequisites/)

## 开发环境

```bash
# 安装前端依赖
pnpm install

# 启动Tauri开发环境
pnpm run tauri dev

# 仅启动Vite开发环境（可以看看UI就行了）
pnpm run dev
```

## 打包构建

```bash
pnpm run tauri build
```

# 注意

本程序为Tauri框架设计的应用，严重依赖WebView，但是程序可以以单文件运行，但是程序仍然会释放一些目录在程序同级目录下。

**法律声明**：本项目在线歌词功能仅用于技术研究。本软件不提供、不存储任何受版权保护的音乐文件。请在当地法律允许的范围内使用。

## 许可证

本项目采用 [GNU General Public License v3.0](LICENSE) 许可证。

### 许可证要求

根据GPL-3.0许可证的要求：

- 您可以自由使用、修改和分发本软件
- 如果您修改了本软件，您必须将修改后的源代码也以GPL-3.0许可证发布
- 本软件不提供任何担保
- 完整的许可证文本请查看 [LICENSE](LICENSE) 文件或访问 [GNU 官网](https://www.gnu.org/licenses/gpl-3.0.html)

### 随软件分发的第三方资源

本项目为GPL-3.0许可证，以下资源以各自的许可证随软件分发（与GPL-3.0兼容，且适用于各自的条款）：

| 资源 | 用途 | 许可证 | 许可证文本 |
| --- | --- | --- | --- |
| [Noto Sans SC](https://github.com/google/fonts/tree/main/ofl/notosanssc)（含 VF，仓库内为 WOFF2 格式转换） | 全局默认字体 / 歌词字体 | SIL OFL 1.1 | [src/assets/fonts/lyrics/OFL-NotoSansSC.txt](src/assets/fonts/lyrics/OFL-NotoSansSC.txt) |
| [霞鹜文楷屏幕版 LXGW WenKai Screen](https://github.com/lxgw/LxgwWenKai-Screen) | 内置歌词字体 | SIL OFL 1.1 | 见上游仓库 |
| [Material Symbols Rounded](https://github.com/google/material-design-icons) | 图标字体 | Apache-2.0 | 见上游仓库 |
| [Roboto](https://github.com/googlefonts/roboto)（@fontsource 自托管） | 可选歌词字体 | Apache-2.0 | 见上游仓库 |

# 致谢

本项目的部分实现参考了以下项目

**歌词获取逻辑参考**

[![LDDC](https://gh-card.dev/repos/chenmozhijin/LDDC.svg)](https://github.com/chenmozhijin/LDDC)
