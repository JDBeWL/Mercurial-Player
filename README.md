![light-virtview-pic](/RISE-darkpink-light-virtview-ass.png)
![dark-modlyrics-pic](/ツキノカメ-blue-dark-modlyrics-ass.png)
![light-classic-pic](/BUZZ_CUTZ-blue-light-classiclyrics-lrc.png)
> 如你所见，这就是一个非常简单的音乐播放器，部分功能未经验证其高可靠性。

# 功能特性

## 音频播放
- [x] 支持格式：MP3、FLAC、WAV（8/16/24/32位）的解码（WAV格式才发现问题，测试不完全，OGG、M4A、AAC的格式没有测试）
- [x] Symphonia作为解码器
- [x] 支持切换输出设备
- [x] WASAPI独占模式（直接访问音频设备，获得最佳音质）
- [x] 高采样率支持（最高384kHz）
- [x] 高质量重采样（使用rubato库）

## 歌词功能
- [x] 多格式支持：LRC、ASS
- [x] 自动加载：根据音频文件名自动查找匹配的歌词文件
- [x] 歌词样式：现代风格/经典风格
- [x] 歌词对齐：左/中/右
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
- [x] 子目录扫描（可配置深度）
- [x] 元数据读取（封面、标题、艺术家）
- [x] 按文件夹创建播放列表

## 界面
- [x] 浅色/深色主题
- [x] 主题颜色系统
- [x] Mini模式（固定大小，置顶显示）
- [x] 中文/English

## 配置
- [x] 配置持久化
- [x] 标题提取配置
- [ ] 配置导入/导出

# TODO
- [ ] 更好的字体显示
- [ ] OGG、M4A、AAC格式测试
- [ ] 歌词编辑功能
- [ ] 均衡器

# 部署？
```cmd
pnpm/npm install

# 使用Tauri dev环境
pnpm/npm run tauri dev

#使用Vite dev环境（不建议，因为大部分使用了Tauri的api接口，如果你觉得有帮助的那你可以这样做）
pnpm/npm run dev
```

# 打包
```
npm run tauri build
```

# 注意
程序会在运行的目录下生成一个配置文件夹用于记录配置