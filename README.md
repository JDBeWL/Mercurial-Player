<div align="center" style="font-size: 24px; font-weight: bold;">
   いつも 不器用な私の番だから<br/>
   现在 该总是很笨拙的我出场了<br/>
   笑って 初めて言えたことだから<br/>
   笑一个 因为是一开始就说好的<br/>
   いつか ゴミのような過去も愛したい<br/>
   总有一天 想去爱垃圾一样的过去<br/>
   だって 私はわたしさ トラッシュライフ<br/>
   因为我就是我 垃圾人生
</div>

![light-virtview-pic](/Calling-pink-dark-virtview-ass.png)
![light-modlyrics-pic](/TRASH_LIFE-blue-light-modlryris.png)

<h3 align="center">
   一款基于TauriVue + TypeScript + Rust开发的音乐播放器。
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
- [x] 切换设备：支持在断开，手动切换下自动切换输出设备，WASAPI独占模式也能通过监听事件实现在共享模式和独占模式的切换，不需要切换下一首，无论是在播放中还是没有播放情况下这个功能基本可用。（未长时间测试）
- [x] 高采样率支持：Roboto能提供什么样的重采样就大概有什么采样
- [x] EQ均衡器
- [x] 淡入淡出：切歌时平滑过渡（独占模式50ms淡出），pause/resume消除爆音（30ms淡入淡出）
- [x] WASAPI独占模式音频加速解码：Windows下的WASAPI独占模式下特殊支持SIMD处理部分数据，如果不支持会fallback到SSE2加速。在不支持软件模拟的ARM64环境下，或者还有不支持SSE2的64位的桌面X86处理器平台在支持WASAPI且能驱动这个WebView2的Windows环境中（~~按道理任何x86_64的CPU都应该支持这个SSE2吧，如果有当我什么都没说~~），这种情况将fallback到不加速

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

## 播放列表
- [x] 文件夹扫描
- [x] 子目录扫描
- [x] 元数据读取（单次采样精度、比特率、封面、标题、艺术家）
- [x] 按文件夹创建播放列表
- [x] 批量元数据获取优化

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
- [x] 更好的字体显示
- [x] 简化图标字体集

# 技术栈

## 前端
| 技术 | 版本 |
|------|------|
| Vue | ^3.5.33 |
| Vite | ^8.0.10 |
| Pinia | ^4.0.2 |
| Vue I18n | ^11.4.6 |
| Sass | ^1.99.0 |
| TypeScript | ^6.0.3 |
| Tauri API | ^2.11.1 |
| @vitejs/plugin-vue | ^6.0.6 |
| Vitest | ^4.1.5 |
| esbuild | ^0.28.0 |
| @material/material-color-utilities | ^0.4.0 |

## 后端 (Rust)
| 技术 | 版本 | 说明 |
|------|------|------|
| Rust | 1.92+ |
| Tauri | 2.11 |
| Symphonia | 0.6 | 音频解码器 |
| Rodio | 0.22 | 音频播放引擎 |
| CPAL | 0.17 |
| WASAPI | 0.23 | Windows独占模式音频 |
| Windows API | 0.62 | Win32 API绑定 |
| Rubato | 4.0 | 音频重采样 |
| Lofty | 0.24 | 音频元数据读取 |
| Tokio | 1.x |
| Reqwest | 0.12 |
| Spectrum Analyzer | 1.7 | 频谱分析 |

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
# 或
npm install

# 启动Tauri开发环境
pnpm run tauri dev
# 或
npm run tauri dev

# 仅启动Vite开发环境（可以看看UI就行了）
npm run dev
```

## 打包构建

```bash
npm run tauri build
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

# 致谢

本项目的部分实现参考了以下项目

**歌词获取逻辑参考**

[![LDDC](https://gh-card.dev/repos/chenmozhijin/LDDC.svg)](https://github.com/chenmozhijin/LDDC)