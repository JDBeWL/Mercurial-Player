/**
 * 歌词截图分享插件
 * 生成精美的歌词分享图片，支持保存和复制到剪贴板
 *
 */

export default function (api) {
  // 默认配置
  const defaultConfig = {
  width: 960,
  height: 1920,
  padding: 80,
  showCover: true,
  showProgress: true,
  coverBlur: 30,
  coverOpacity: 0.4,
  fontFamily: 'system-ui, -apple-system, sans-serif',
  layout: 'classic', // 'classic' (经典布局) 或 'compact' (紧凑布局)
}

/**
 * 获取配置
 */
const getConfig = () => {
  return { ...defaultConfig, ...api.storage.get('config', {}) }
}

/**
 * 绘制圆角矩形路径（使用原生 Canvas API，Chrome 99+/Safari 17+ 支持）
 * 自动 beginPath，调用方直接 fill() 或 clip() 即可
 */
const roundRect = (ctx, x, y, w, h, r) => {
  ctx.beginPath()
  ctx.roundRect(x, y, w, h, r)
}

/**
 * 自动换行绘制文本，返回绘制信息
 * @param {boolean} dryRun - 如果为 true，只计算不绘制
 */
const wrapText = (ctx, text, x, y, maxWidth, lineHeight, dryRun = false) => {
  const chars = text.split('')
  let line = ''
  let lines = []

  for (let i = 0; i < chars.length; i++) {
    const testLine = line + chars[i]
    const metrics = ctx.measureText(testLine)
    if (metrics.width > maxWidth && line.length > 0) {
      lines.push(line)
      line = chars[i]
    } else {
      line = testLine
    }
  }
  lines.push(line)

  const totalHeight = lines.length * lineHeight
  const startY = y - totalHeight / 2 + lineHeight / 2

  if (!dryRun) {
    lines.forEach((l, idx) => {
      ctx.fillText(l, x, startY + idx * lineHeight)
    })
  }

  return {
    totalHeight,
    lines: lines.length,
    startY,
    endY: startY + (lines.length - 1) * lineHeight,
  }
}

/**
 * 截断文本并添加省略号
 */
const truncateText = (ctx, text, maxWidth) => {
  if (ctx.measureText(text).width <= maxWidth) {
    return text
  }
  let truncated = text
  while (ctx.measureText(truncated + '...').width > maxWidth && truncated.length > 0) {
    truncated = truncated.slice(0, -1)
  }
  return truncated + '...'
}

// 让出主线程的辅助函数
const yieldToMain = () => new Promise((resolve) => setTimeout(resolve, 0))

/**
 * 从歌词数据中提取当前歌词文本和翻译
 * 抽取自 generateClassicImage 和 generateCompactImage 的公共逻辑
 */
const getCurrentLyricText = (lyrics, lyricIndex) => {
  let mainText = ''
  let transText = ''

  if (lyrics && lyrics.length > 0) {
    let currentLyric = null
    if (lyricIndex >= 0 && lyricIndex < lyrics.length) {
      currentLyric = lyrics[lyricIndex]
    } else {
      currentLyric = lyrics[0]
    }
    if (currentLyric) {
      const firstText = currentLyric.texts?.[0]
      const secondText = currentLyric.texts?.[1]

      if (typeof firstText === 'object' && firstText !== null) {
        mainText = firstText.text || ''
      } else if (typeof firstText === 'string') {
        mainText = firstText
      } else if (currentLyric.text) {
        mainText = currentLyric.text
      }

      if (typeof secondText === 'object' && secondText !== null) {
        transText = secondText.text || ''
      } else if (typeof secondText === 'string') {
        transText = secondText
      } else if (currentLyric.translation) {
        transText = currentLyric.translation
      }
    }
  }

  return { mainText, transText }
}

/**
 * 获取主题颜色（抽取自两个生成函数的公共逻辑）
 * @param {boolean} isDark - 是否深色模式，用于回退颜色
 */
const getThemeColors = (isDark = false) => {
  return {
    bgColor:
      api.theme.getCSSVariable('md-sys-color-background') ||
      (isDark ? '#121212' : '#fefefe'),
    primaryColor:
      api.theme.getCSSVariable('md-sys-color-primary') || '#6750a4',
    onBgColor:
      api.theme.getCSSVariable('md-sys-color-on-background') ||
      (isDark ? '#e6e1e5' : '#1c1b1f'),
    onSurfaceVariant:
      api.theme.getCSSVariable('md-sys-color-on-surface-variant') ||
      (isDark ? '#cac4d0' : '#49454f'),
    surfaceContainer:
      api.theme.getCSSVariable('md-sys-color-surface-container') ||
      (isDark ? '#211f26' : '#f3edf7'),
  }
}

/**
 * 加载封面图片
 * 使用 api.player.getCoverPath() 直接从后端获取封面路径，
 * 不依赖 store 的异步加载时序。
 * @returns {Promise<HTMLImageElement|null>}
 */
const loadCoverImage = async () => {
  const coverPath = await api.player.getCoverPath()

  if (!coverPath) {
    api.log.warn('未能获取到封面路径，将使用占位符')
    return null
  }

  try {
    return await api.utils.loadImage(coverPath)
  } catch (e) {
    api.log.debug('封面加载失败:', e)
    return null
  }
}

/**
 * 绘制模糊封面背景（抽取自两个生成函数的公共逻辑）
 * @param {Array<[number, string, string]>} [gradientStops] - 渐变遮罩停止点
 *   格式: [[offset, darkColor, lightColor], ...]，默认使用经典布局的遮罩
 */
const drawBlurredBackground = (ctx, coverImg, width, height, config, isDark, gradientStops) => {
  if (!config.showCover || !coverImg) return

  ctx.save()
  ctx.globalAlpha = config.coverOpacity
  ctx.filter = `blur(${config.coverBlur}px)`

  const scale = Math.max(width / coverImg.width, height / coverImg.height) * 1.2
  const scaledW = coverImg.width * scale
  const scaledH = coverImg.height * scale
  const offsetX = (width - scaledW) / 2
  const offsetY = (height - scaledH) / 2

  ctx.drawImage(coverImg, offsetX, offsetY, scaledW, scaledH)
  ctx.restore()

  // 渐变遮罩
  const stops = gradientStops || [
    [0, 'rgba(18,18,18,0.7)', 'rgba(254,254,254,0.7)'],
    [0.5, 'rgba(18,18,18,0.5)', 'rgba(254,254,254,0.5)'],
    [1, 'rgba(18,18,18,0.9)', 'rgba(254,254,254,0.9)'],
  ]
  const gradient = ctx.createLinearGradient(0, 0, 0, height)
  for (const [offset, darkColor, lightColor] of stops) {
    gradient.addColorStop(offset, isDark ? darkColor : lightColor)
  }
  ctx.fillStyle = gradient
  ctx.fillRect(0, 0, width, height)
}

/**
 * 计算文本行数（不绘制，用于高度预估）
 */
const calculateLineCount = (ctx, text, maxWidth) => {
  if (!text) return 0
  let lineCount = 1
  let line = ''
  for (const char of text) {
    const testLine = line + char
    if (ctx.measureText(testLine).width > maxWidth && line.length > 0) {
      lineCount++
      line = char
    } else {
      line = testLine
    }
  }
  return lineCount
}

/**
 * 生成经典布局分享图片
 */
const generateClassicImage = async (options = {}) => {
  const config = { ...getConfig(), ...options }
  const { width, padding } = config

  // 先让出主线程，避免阻塞 UI
  await yieldToMain()

  const state = await api.player.getState()
  const lyrics = await api.player.getLyrics()
  const lyricIndex = await api.player.getCurrentLyricIndex()

  // 调试日志
  api.log.info('生成图片 - 歌曲:', state.currentTrack?.title)
  api.log.info('生成图片 - 歌词数量:', lyrics?.length || 0)
  api.log.info('生成图片 - 歌词索引:', lyricIndex)

  if (!state.currentTrack) {
    api.ui.showNotification('没有正在播放的歌曲', 'warning')
    return null
  }

  // 获取当前歌词（使用公共抽取函数）
  const { mainText, transText } = getCurrentLyricText(lyrics, lyricIndex)

  // 让出主线程，避免长时间阻塞
  await yieldToMain()

  // 计算歌词需要的行数来决定高度（使用公共计算函数）
  const tempCanvas = api.utils.createCanvas(width, 100)
  const tempCtx = tempCanvas.ctx
  const maxLyricWidth = width - padding * 2

  tempCtx.font = `bold 56px ${config.fontFamily}`
  const mainLines = calculateLineCount(tempCtx, mainText, maxLyricWidth)

  tempCtx.font = `52px ${config.fontFamily}`
  const transLines = calculateLineCount(tempCtx, transText, maxLyricWidth)

  // 动态计算高度
  const coverSize = Math.min(width - padding * 2, 400)
  const hasLyrics = mainText.length > 0

  // 基础高度：padding + 封面区域 + 歌曲信息 + 进度条 + padding
  const baseHeight = padding + 60 + coverSize + 60 + 80 + 150 + padding

  // 歌词区域高度（没有歌词时为 0）
  const lyricAreaHeight = hasLyrics
    ? 100 + mainLines * 72 + (transText ? transLines * 64 + 30 : 0)
    : 0

  const height = Math.max(hasLyrics ? 1280 : 900, Math.min(1920, baseHeight + lyricAreaHeight))

  // 获取主题颜色
  const themeInfo = await api.theme.getCurrent()
  const isDark = themeInfo.isDark

  // 创建画布
  const { canvas, ctx } = api.utils.createCanvas(width, height)

  // 获取主题颜色（使用公共抽取函数）
  const { bgColor, primaryColor, onBgColor, onSurfaceVariant, surfaceContainer } =
    getThemeColors(isDark)

  // 绘制纯色背景
  ctx.fillStyle = bgColor
  ctx.fillRect(0, 0, width, height)

  // 让出主线程
  await yieldToMain()

  // 预加载封面图片（使用公共加载函数）
  const coverImg = await loadCoverImage()

  // 绘制封面背景（模糊效果，使用公共绘制函数）
  drawBlurredBackground(ctx, coverImg, width, height, config, isDark)

  // 让出主线程
  await yieldToMain()

  // 绘制封面图片（居中显示）
  const coverX = (width - coverSize) / 2
  const coverY = padding + 60

  if (coverImg) {
    // 绘制阴影
    ctx.save()
    ctx.shadowColor = 'rgba(0,0,0,0.3)'
    ctx.shadowBlur = 30
    ctx.shadowOffsetY = 10

    // 圆角封面
    roundRect(ctx, coverX, coverY, coverSize, coverSize, 24)
    ctx.clip()
    ctx.drawImage(coverImg, coverX, coverY, coverSize, coverSize)
    ctx.restore()
  } else {
    // 无封面占位符
    ctx.fillStyle = surfaceContainer
    roundRect(ctx, coverX, coverY, coverSize, coverSize, 24)
    ctx.fill()

    ctx.fillStyle = onSurfaceVariant
    ctx.font = `${coverSize / 3}px ${config.fontFamily}`
    ctx.textAlign = 'center'
    ctx.textBaseline = 'middle'
    ctx.fillText('♪', coverX + coverSize / 2, coverY + coverSize / 2)
  }

  // 歌曲信息区域
  const infoY = coverY + coverSize + 60
  ctx.textAlign = 'center'
  ctx.textBaseline = 'middle'

  // 歌曲标题
  ctx.fillStyle = onBgColor
  ctx.font = `bold 48px ${config.fontFamily}`
  const title = state.currentTrack.title || state.currentTrack.name || '未知歌曲'
  const titleInfo = wrapText(ctx, title, width / 2, infoY, width - padding * 2, 58)

  // 艺术家（支持换行）
  ctx.fillStyle = onSurfaceVariant
  ctx.font = `32px ${config.fontFamily}`
  const artist = state.currentTrack.artist || '未知艺术家'
  const artistY = infoY + titleInfo.totalHeight / 2 + 50
  const artistInfo = wrapText(ctx, artist, width / 2, artistY, width - padding * 2, 42)

  // 歌词区域 - 在封面信息和进度条之间居中
  // 歌词区域的范围：从歌曲信息下方到进度条上方
  const lyricsAreaTop = artistY + artistInfo.totalHeight / 2 + 40 // 艺术家下方
  const lyricsAreaBottom = height - padding - 140 // 进度条上方
  const lyricsAreaCenterY = (lyricsAreaTop + lyricsAreaBottom) / 2
  const gap = 24 // 主歌词和翻译之间的间隙

  if (mainText) {
    // 先计算总高度
    ctx.font = `bold 56px ${config.fontFamily}`
    const mainInfo = wrapText(ctx, mainText, 0, 0, maxLyricWidth, 72, true)

    let transInfo = null
    if (transText) {
      ctx.font = `52px ${config.fontFamily}`
      transInfo = wrapText(ctx, transText, 0, 0, maxLyricWidth, 64, true)
    }

    // 计算整体高度
    const totalLyricHeight = mainInfo.totalHeight + (transInfo ? gap + transInfo.totalHeight : 0)

    // 计算起始 Y 位置，使整体居中
    const lyricStartY = lyricsAreaCenterY - totalLyricHeight / 2 + mainInfo.totalHeight / 2

    // 绘制主歌词
    ctx.fillStyle = primaryColor
    ctx.font = `bold 56px ${config.fontFamily}`
    wrapText(ctx, mainText, width / 2, lyricStartY, maxLyricWidth, 72)

    // 绘制翻译歌词
    if (transText && transInfo) {
      const transY = lyricStartY + mainInfo.totalHeight / 2 + gap + transInfo.totalHeight / 2
      ctx.save()
      ctx.fillStyle = primaryColor
      ctx.globalAlpha = 0.85
      ctx.font = `52px ${config.fontFamily}`
      wrapText(ctx, transText, width / 2, transY, maxLyricWidth, 64)
      ctx.restore()
    }
  }

  // 进度条
  if (config.showProgress && state.duration > 0) {
    const progressY = height - padding - 80
    const progressWidth = width - padding * 2
    const progressHeight = 8
    const progressX = padding

    // 背景条
    ctx.fillStyle = surfaceContainer
    roundRect(ctx, progressX, progressY, progressWidth, progressHeight, 4)
    ctx.fill()

    // 进度条
    const progress = state.currentTime / state.duration
    const currentWidth = progressWidth * progress
    if (currentWidth > 0) {
      ctx.fillStyle = primaryColor
      roundRect(ctx, progressX, progressY, currentWidth, progressHeight, 4)
      ctx.fill()
    }

    // 时间文字
    ctx.fillStyle = onSurfaceVariant
    ctx.font = `24px ${config.fontFamily}`
    ctx.textAlign = 'left'
    ctx.fillText(api.utils.formatTime(state.currentTime), progressX, progressY + 40)
    ctx.textAlign = 'right'
    ctx.fillText(api.utils.formatTime(state.duration), progressX + progressWidth, progressY + 40)
  }

  // 水印
  ctx.fillStyle = onSurfaceVariant
  ctx.globalAlpha = 0.5
  ctx.font = `20px ${config.fontFamily}`
  ctx.textAlign = 'center'
  ctx.fillText('Mercurial Player', width / 2, height - padding / 2)
  ctx.globalAlpha = 1

  return canvas
}

/**
 * 生成紧凑布局分享图片
 * 布局：歌词在上方，封面在右下角，歌曲信息和进度条在左下角
 */
const generateCompactImage = async (options = {}) => {
  const config = { ...getConfig(), ...options }
  const { width, padding } = config

  // 先让出主线程，避免阻塞 UI
  await yieldToMain()

  const state = await api.player.getState()
  const lyrics = await api.player.getLyrics()
  const lyricIndex = await api.player.getCurrentLyricIndex()

  // 调试日志
  api.log.info('生成紧凑布局图片 - 歌曲:', state.currentTrack?.title)

  if (!state.currentTrack) {
    api.ui.showNotification('没有正在播放的歌曲', 'warning')
    return null
  }

  // 获取当前歌词（使用公共抽取函数）
  const { mainText, transText } = getCurrentLyricText(lyrics, lyricIndex)

  await yieldToMain()

  // 紧凑布局尺寸
  const coverSize = 200 // 右下角封面尺寸较小
  const bottomAreaHeight = 280 // 底部区域高度（封面+信息+进度条）
  const hasLyrics = mainText.length > 0

  // 计算歌词行数（使用公共计算函数）
  const maxLyricWidth = width - padding * 2
  const tempCanvas = api.utils.createCanvas(width, 100)
  const tempCtx = tempCanvas.ctx

  tempCtx.font = `bold 56px ${config.fontFamily}`
  const mainLines = calculateLineCount(tempCtx, mainText, maxLyricWidth)

  tempCtx.font = `52px ${config.fontFamily}`
  const transLines = calculateLineCount(tempCtx, transText, maxLyricWidth)

  // 动态计算高度
  const lyricAreaHeight = hasLyrics
    ? padding + mainLines * 72 + (transText ? transLines * 64 + 30 : 0) + 60
    : 200

  const height = Math.max(800, Math.min(1600, lyricAreaHeight + bottomAreaHeight + padding))

  // 获取主题颜色
  const themeInfo = await api.theme.getCurrent()
  const isDark = themeInfo.isDark

  // 创建画布
  const { canvas, ctx } = api.utils.createCanvas(width, height)

  // 获取主题颜色（使用公共抽取函数）
  const { bgColor, primaryColor, onBgColor, onSurfaceVariant, surfaceContainer } =
    getThemeColors(isDark)

  // 绘制纯色背景
  ctx.fillStyle = bgColor
  ctx.fillRect(0, 0, width, height)

  await yieldToMain()

  // 预加载封面图片（使用公共加载函数）
  const coverImg = await loadCoverImage()

  // 绘制封面背景（模糊效果，使用公共绘制函数，紧凑布局使用更深的底部遮罩）
  drawBlurredBackground(ctx, coverImg, width, height, config, isDark, [
    [0, 'rgba(18,18,18,0.6)', 'rgba(254,254,254,0.6)'],
    [0.7, 'rgba(18,18,18,0.7)', 'rgba(254,254,254,0.7)'],
    [1, 'rgba(18,18,18,0.95)', 'rgba(254,254,254,0.95)'],
  ])

  await yieldToMain()

  // ========== 绘制歌词区域（上方） ==========
  const lyricsAreaTop = padding + 40
  const lyricsAreaBottom = height - bottomAreaHeight - 20
  const lyricsAreaCenterY = (lyricsAreaTop + lyricsAreaBottom) / 2
  const gap = 24

  if (mainText) {
    ctx.font = `bold 56px ${config.fontFamily}`
    const mainInfo = wrapText(ctx, mainText, 0, 0, maxLyricWidth, 72, true)

    let transInfo = null
    if (transText) {
      ctx.font = `52px ${config.fontFamily}`
      transInfo = wrapText(ctx, transText, 0, 0, maxLyricWidth, 64, true)
    }

    const totalLyricHeight = mainInfo.totalHeight + (transInfo ? gap + transInfo.totalHeight : 0)
    const lyricStartY = lyricsAreaCenterY - totalLyricHeight / 2 + mainInfo.totalHeight / 2

    // 绘制主歌词
    ctx.fillStyle = primaryColor
    ctx.font = `bold 56px ${config.fontFamily}`
    ctx.textAlign = 'center'
    ctx.textBaseline = 'middle'
    wrapText(ctx, mainText, width / 2, lyricStartY, maxLyricWidth, 72)

    // 绘制翻译歌词
    if (transText && transInfo) {
      const transY = lyricStartY + mainInfo.totalHeight / 2 + gap + transInfo.totalHeight / 2
      ctx.save()
      ctx.fillStyle = primaryColor
      ctx.globalAlpha = 0.85
      ctx.font = `52px ${config.fontFamily}`
      wrapText(ctx, transText, width / 2, transY, maxLyricWidth, 64)
      ctx.restore()
    }
  }

  // ========== 底部区域 ==========
  // 布局：
  // - 封面：右下角，200x200
  // - 左侧从上到下：进度条 -> 时间 -> 标题 -> 艺术家

  // ========== 绘制封面（右下角） ==========
  const coverX = width - padding - coverSize
  const coverY = height - padding - coverSize

  if (coverImg) {
    ctx.save()
    ctx.shadowColor = 'rgba(0,0,0,0.25)'
    ctx.shadowBlur = 20
    ctx.shadowOffsetY = 6

    roundRect(ctx, coverX, coverY, coverSize, coverSize, 16)
    ctx.clip()
    ctx.drawImage(coverImg, coverX, coverY, coverSize, coverSize)
    ctx.restore()
  } else {
    ctx.fillStyle = surfaceContainer
    roundRect(ctx, coverX, coverY, coverSize, coverSize, 16)
    ctx.fill()

    ctx.fillStyle = onSurfaceVariant
    ctx.font = `${coverSize / 3}px ${config.fontFamily}`
    ctx.textAlign = 'center'
    ctx.textBaseline = 'middle'
    ctx.fillText('♪', coverX + coverSize / 2, coverY + coverSize / 2)
  }

  // ========== 绘制左侧信息区域 ==========
  const infoMaxWidth = coverX - padding - 30 // 留出封面和间距
  const infoX = padding

  // 计算左侧区域的垂直位置（与封面对齐）
  const leftAreaTop = coverY // 与封面顶部对齐
  const leftAreaBottom = coverY + coverSize // 与封面底部对齐

  // 进度条在顶部
  if (config.showProgress && state.duration > 0) {
    const progressY = leftAreaTop + 10
    const progressWidth = infoMaxWidth
    const progressHeight = 6
    const progressX = padding

    // 背景条
    ctx.fillStyle = surfaceContainer
    roundRect(ctx, progressX, progressY, progressWidth, progressHeight, 3)
    ctx.fill()

    // 进度条
    const progress = state.currentTime / state.duration
    const currentWidth = progressWidth * progress
    if (currentWidth > 0) {
      ctx.fillStyle = primaryColor
      roundRect(ctx, progressX, progressY, currentWidth, progressHeight, 3)
      ctx.fill()
    }

    // 时间文字
    ctx.fillStyle = onSurfaceVariant
    ctx.font = `18px ${config.fontFamily}`
    ctx.textAlign = 'left'
    ctx.textBaseline = 'top'
    ctx.fillText(api.utils.formatTime(state.currentTime), progressX, progressY + 14)
    ctx.textAlign = 'right'
    ctx.fillText(api.utils.formatTime(state.duration), progressX + progressWidth, progressY + 14)
  }

  // 歌曲标题（在底部区域的中下部）
  const titleY = leftAreaBottom - 60 // 距离底部60px
  ctx.fillStyle = onBgColor
  ctx.font = `bold 40px ${config.fontFamily}`
  ctx.textAlign = 'left'
  ctx.textBaseline = 'bottom'
  const title = state.currentTrack.title || state.currentTrack.name || '未知歌曲'
  const titleTruncated = truncateText(ctx, title, infoMaxWidth)
  ctx.fillText(titleTruncated, infoX, titleY)

  // 艺术家（在标题下方）
  ctx.fillStyle = onSurfaceVariant
  ctx.font = `28px ${config.fontFamily}`
  ctx.textBaseline = 'top'
  const artist = state.currentTrack.artist || '未知艺术家'
  const artistTruncated = truncateText(ctx, artist, infoMaxWidth)
  ctx.fillText(artistTruncated, infoX, titleY + 16)

  // 水印
  ctx.fillStyle = onSurfaceVariant
  ctx.globalAlpha = 0.4
  ctx.font = `18px ${config.fontFamily}`
  ctx.textAlign = 'center'
  ctx.textBaseline = 'top'
  ctx.fillText('Mercurial Player', width / 2, padding / 2)
  ctx.globalAlpha = 1

  return canvas
}

/**
 * 生成分享图片（入口函数，根据配置选择布局）
 */
const generateImage = async (options = {}) => {
  const config = { ...getConfig(), ...options }

  if (config.layout === 'compact') {
    return generateCompactImage(options)
  }
  return generateClassicImage(options)
}

let isGenerating = false

// 插件主体
const plugin = {
  activate() {
    api.log.info('歌词截图分享插件已激活')

    // 注册快捷键 - 统一操作
    // 注意：避免使用 Ctrl+Shift+C/S，这些在 WebView2 中会被 DevTools/另存为拦截
    api.shortcuts.register({
      id: 'lyrics-share-copy',
      name: '复制歌词图片',
      key: 'Alt+Shift+C',
      description: '将当前歌词生成分享图片并复制到剪贴板',
      action: () => this.copyImage(),
    })

    api.shortcuts.register({
      id: 'lyrics-share-save',
      name: '保存歌词图片',
      key: 'Alt+Shift+S',
      description: '将当前歌词生成分享图片并保存到本地',
      action: () => this.saveImage(),
    })

    // 注册操作按钮 - 布局切换
    api.ui.registerActionButton({
      id: 'lyrics-share-toggle-layout-btn',
      name: `切换布局 (当前: ${getConfig().layout === 'compact' ? '紧凑' : '经典'})`,
      icon: 'dashboard',
      location: 'lyrics',
      action: () => this.toggleLayout(),
    })

    // 注册操作按钮 - 统一操作
    api.ui.registerActionButton({
      id: 'lyrics-share-copy-btn',
      name: '复制图片 (Alt+Shift+C)',
      icon: 'content_copy',
      location: 'lyrics',
      action: () => this.copyImage(),
    })

    api.ui.registerActionButton({
      id: 'lyrics-share-save-btn',
      name: '保存图片 (Alt+Shift+S)',
      icon: 'save',
      location: 'lyrics',
      action: () => this.saveImage(),
    })
  },

  deactivate() {
    // 取消注册快捷键
    api.shortcuts.unregister('lyrics-share-copy')
    api.shortcuts.unregister('lyrics-share-save')

    // 取消注册按钮
    api.ui.unregisterActionButton('lyrics-share-copy-btn')
    api.ui.unregisterActionButton('lyrics-share-save-btn')
    api.ui.unregisterActionButton('lyrics-share-toggle-layout-btn')
    api.log.info('歌词截图分享插件已停用')
  },

  /**
   * 生成并保存图片
   */
  async saveImage(options = {}) {
    // 防止重复执行导致页面冻结
    if (isGenerating) {
      api.log.warn('图片正在生成中，请稍候...')
      return null
    }

    isGenerating = true
    try {
      // 使用 setTimeout 让 UI 有机会更新
      await new Promise((resolve) => setTimeout(resolve, 10))

      const canvas = await generateImage(options)
      if (!canvas) return null

      const timestamp = new Date().toISOString().replace(/[:.]/g, '-').slice(0, 19)
      const state = await api.player.getState()
      const trackName = (state.currentTrack?.title || 'lyrics').replace(/[<>:"/\\|?*]/g, '_')
      const fileName = `${trackName}-${timestamp}.png`

      const path = await api.file.saveImage(canvas, fileName, 'png')
      if (path) {
        api.ui.showNotification(`图片已保存到 screenshots 目录`, 'info')
        // 自动打开目录
        await api.file.openScreenshotsDirectory()
        return path
      }
      return null
    } catch (error) {
      api.log.error('保存图片失败:', error)
      api.ui.showNotification('保存失败: ' + error.message, 'error')
      return null
    } finally {
      isGenerating = false
    }
  },

  /**
   * 生成并复制到剪贴板
   */
  async copyImage(options = {}) {
    // 防止重复执行导致页面冻结
    if (isGenerating) {
      api.log.warn('图片正在生成中，请稍候...')
      return false
    }

    isGenerating = true
    try {
      // 使用 setTimeout 让 UI 有机会更新
      await new Promise((resolve) => setTimeout(resolve, 10))

      const canvas = await generateImage(options)
      if (!canvas) return false

      await api.clipboard.writeImage(canvas)
      api.ui.showNotification('图片已复制到剪贴板', 'info')
      return true
    } catch (error) {
      api.log.error('复制图片失败:', error)
      api.ui.showNotification('复制失败: ' + error.message, 'error')
      return false
    } finally {
      isGenerating = false
    }
  },

  /**
   * 获取图片 DataURL（用于预览）
   */
  async getImageDataURL(options = {}) {
    const canvas = await generateImage(options)
    if (!canvas) return null
    return api.utils.canvasToDataURL(canvas, 'image/png')
  },

  /**
   * 更新配置
   */
  setConfig(newConfig) {
    const config = getConfig()
    api.storage.set('config', { ...config, ...newConfig })
  },

  /**
   * 获取当前配置
   */
  getConfig() {
    return getConfig()
  },

  /**
   * 切换布局
   */
  toggleLayout() {
    const config = getConfig()
    const newLayout = config.layout === 'compact' ? 'classic' : 'compact'
    api.storage.set('config', { ...config, layout: newLayout })

    const layoutName = newLayout === 'compact' ? '紧凑' : '经典'
    api.ui.showNotification(`已切换到${layoutName}布局`, 'info')
    api.log.info(`布局已切换为: ${newLayout}`)
  },
}

  return plugin
}
