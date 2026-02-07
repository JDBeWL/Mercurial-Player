![light-virtview-pic](/RISE-pink-light-virtview-ass.png)
![dark-modlyrics-pic](/ツキノカメ-blue-dark-modlyrics-ass.png)
![light-classic-pic](/BUZZ_CUTZ-blue-light-classiclyrics-lrc.png)

> 这是一款基于Tauri v2开发的音乐播放器，专注于高质量音频输出与极致的歌词显示体验。

> *注意：部分功能（如WASAPI独占模式）正处于开发阶段，可能存在不稳定性。*
>
> *没有引入FFmpeg到项目中，如果文件头没有对应的特征头将解不开文件，比如0x8400前面全是没有用的数据还封装成aac格式让人迷惑的ADTS流或者用LOAS封装的神必流。*

# 功能特性

## 音频播放
- [x] 支持格式：MP3、FLAC、WAV（8/16/24/32位）的解码（AAC、OGG、M4A的格式没有严格测试）
- [x] 使用Symphonia作为解码器
- [x] 支持自动切换输出设备
- [x] WASAPI独占模式（抢设备会重复去抢到独占模式，如果失败多次抛出错误，Windows下的直接访问音频设备，尽量避免混音给音频带来的影响）
- [x] 高采样率支持
- [x] 高质量重采样

## 歌词功能
- [x] 多格式支持：LRC、ASS
- [x] 自动加载：根据音频文件名自动查找匹配的歌词文件
- [x] 在线歌词：从网易云音乐Web API获取歌词（这个会再改改但是基本能用）
- [x] 歌词样式：现代风格/经典风格
- [x] 歌词对齐：左/中/右
- [x] 歌词偏移
- [x] 点击歌词跳转
- [x] 卡拉OK逐字高亮（ASS格式）
- [x] 双语歌词显示

## 播放控制
- [x] 播放/暂停/上一首/下一首
- [x] 进度条拖动跳转
- [x] 音量调节
- [x] 单曲循环/列表循环
- [x] 随机播放

## 可视化
- [x] 实时FFT频谱（共享模式下可用）
- [x] 60fps的动画
- [x] 歌词/可视化视图切换

## 播放列表
- [x] 文件夹扫描
- [x] 子目录扫描
- [x] 元数据读取（封面、标题、艺术家）
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

# 技术栈

## 前端
| 技术 | 版本 |
|------|------|
| Vue | ^3.3.4 |
| Vite | ^6.0.0 |
| Pinia | ^2.1.6 |
| Vue I18n | ^9.14.5 |
| Sass | ^1.64.2 |
| TypeScript | ^5.9.3 |
| Tauri API | ^2.9.1 |
| @vitejs/plugin-vue | ^5.0.0 |
| Vitest | ^4.0.16 |

## 后端 (Rust)
| 技术 | 版本 | 说明 |
|------|------|------|
| Rust | 1.92+ |
| Tauri | 2.9 |
| Symphonia | 0.5 | 音频解码器 |
| Rodio | 0.21 | 音频播放引擎 |
| CPAL | 0.16 |
| WASAPI | 0.22 | Windows独占模式音频 |
| Windows API | 0.61 | Win32 API绑定 |
| Rubato | 0.15 | 音频重采样 |
| Lofty | 0.22 | 音频元数据读取 |
| Tokio | 1.x |
| Reqwest | 0.12 |
| Spectrum Analyzer | 1.7 | 频谱分析 |

# 部署

## 环境要求

1. **Node.js** - 推荐v18+
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