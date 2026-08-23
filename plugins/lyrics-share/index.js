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
 * 歌词排版基准字号（两种布局共用，自适应缩放的基准）
 */
const LYRIC_BASE_FONTS = { main: 56, trans: 52, mainLineH: 72, transLineH: 64, gap: 24 }

/**
 * 按缩放比例测量歌词块尺寸（含换行重排），不绘制
 * @returns {{mainFont:number,transFont:number,mainLineH:number,transLineH:number,gap:number,height:number,mainLines:number,transLines:number}}
 */
const measureLyricBlock = (ctx, mainText, transText, maxWidth, scale, fontFamily) => {
  const mainFont = Math.round(LYRIC_BASE_FONTS.main * scale)
  const transFont = Math.round(LYRIC_BASE_FONTS.trans * scale)
  const mainLineH = Math.round(LYRIC_BASE_FONTS.mainLineH * scale)
  const transLineH = Math.round(LYRIC_BASE_FONTS.transLineH * scale)
  const gap = Math.round(LYRIC_BASE_FONTS.gap * scale)

  ctx.font = `bold ${mainFont}px ${fontFamily}`
  const mainLines = mainText ? calculateLineCount(ctx, mainText, maxWidth) : 0

  let transLines = 0
  if (transText) {
    ctx.font = `${transFont}px ${fontFamily}`
    transLines = calculateLineCount(ctx, transText, maxWidth)
  }

  const height =
    mainLines * mainLineH + (transText && transLines > 0 ? gap + transLines * transLineH : 0)

  return { mainFont, transFont, mainLineH, transLineH, gap, height, mainLines, transLines }
}

/**
 * 在 [minScale, maxScale] 区间内二分搜索能放入 areaHeight 的最大歌词缩放比例。
 * 空间富余时放大字号填充（不超过 maxScale），空间不足时缩小（不低于 minScale）。
 * @returns 测量结果；返回 null 表示 minScale 也放不下（调用方需要加高画布）
 */
const fitLyricScale = (
  ctx,
  mainText,
  transText,
  maxWidth,
  areaHeight,
  fontFamily,
  minScale = 0.65,
  maxScale = 1.5,
) => {
  // 目标高度留 8% 呼吸空间，避免文字贴边
  const target = areaHeight * 0.92

  let best = measureLyricBlock(ctx, mainText, transText, maxWidth, minScale, fontFamily)
  if (best.height > target) return null

  let lo = minScale
  let hi = maxScale
  for (let i = 0; i < 9; i++) {
    const mid = (lo + hi) / 2
    const m = measureLyricBlock(ctx, mainText, transText, maxWidth, mid, fontFamily)
    if (m.height <= target) {
      best = m
      lo = mid
    } else {
      hi = mid
    }
  }
  return best
}

/**
 * 生成经典布局分享图片
 * 自适应排版：先在临时画布上完成全部测量，空间富余时放大封面/标题/歌词填满画布，
 * 空间不足时缩小歌词字号防止溢出（极端情况加高画布兜底）
 */
const generateClassicImage = async (options = {}) => {
  const config = { ...getConfig(), ...options }
  const { width, padding } = config
  const contentWidth = width - padding * 2

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
  const hasLyrics = mainText.length > 0

  // 让出主线程，避免长时间阻塞
  await yieldToMain()

  // ==== 自适应排版：先测量，定稿尺寸后再绘制 ====
  const tempCanvas = api.utils.createCanvas(width, 100)
  const tempCtx = tempCanvas.ctx

  const title = state.currentTrack.title || state.currentTrack.name || '未知歌曲'
  const artist = state.currentTrack.artist || '未知艺术家'

  // 基准字号下的歌词块高度
  const baseMeasure = measureLyricBlock(tempCtx, mainText, transText, contentWidth, 1, config.fontFamily)

  // 垂直方向固定开销：顶部留白60 + 封面下间距60 + 歌词上留白40 + 进度区140
  const FIXED_V = 300
  // 基准信息区高度（标题58 + 间距50 + 艺人42，单行估算）
  const BASE_INFO_H = 150
  const baseCoverSize = Math.min(contentWidth, 400)

  // 基准状态下的自然内容高度
  const naturalHeight = padding * 2 + FIXED_V + baseCoverSize + BASE_INFO_H + baseMeasure.height

  // 画布高度：沿用原有上下限（有歌词 1280 ~ 1920，无歌词 900 ~ 1920）
  let height = Math.max(hasLyrics ? 1280 : 900, Math.min(1920, naturalHeight))

  // ---- 分配富余空间：放大封面，标题/艺人字号小幅跟随 ----
  const extra = height - naturalHeight
  let coverSize = baseCoverSize
  let titleScale = 1
  if (extra > 0) {
    coverSize = baseCoverSize + Math.min(extra * 0.55, contentWidth - baseCoverSize)
    titleScale = Math.min(1.18, 1 + extra * 0.0008)
  }

  const titleFont = Math.round(48 * titleScale)
  const artistFont = Math.round(32 * titleScale)
  const titleLineH = Math.round(58 * titleScale)
  const artistLineH = Math.round(42 * titleScale)
  const titleGap = Math.round(50 * titleScale)

  // 放大后重新测量标题/艺人实际行数
  tempCtx.font = `bold ${titleFont}px ${config.fontFamily}`
  const titleLines = calculateLineCount(tempCtx, title, contentWidth)
  tempCtx.font = `${artistFont}px ${config.fontFamily}`
  const artistLines = calculateLineCount(tempCtx, artist, contentWidth)
  const infoBlockH = titleLines * titleLineH + titleGap + artistLines * artistLineH

  // ---- 歌词区域：按实际剩余空间自适应缩放 ----
  const lyricAreaTop = padding + 60 + coverSize + 60 + infoBlockH + 40
  const lyricAreaBottom = height - padding - 140

  let lyricMeasure = null
  if (hasLyrics) {
    lyricMeasure = fitLyricScale(
      tempCtx,
      mainText,
      transText,
      contentWidth,
      lyricAreaBottom - lyricAreaTop,
      config.fontFamily,
    )
    if (!lyricMeasure) {
      // 最小字号也放不下：加高画布承接全部歌词，避免文字溢出
      lyricMeasure = measureLyricBlock(tempCtx, mainText, transText, contentWidth, 0.65, config.fontFamily)
      height = lyricAreaTop + lyricMeasure.height + 40 + 140 + padding
    }
  }

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

  // ==== 绘制封面（居中，按富余空间放大） ====
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

  // ==== 绘制歌曲信息（标题 + 艺人，流式排布，字号随富余空间放大） ====
  ctx.textAlign = 'center'
  ctx.textBaseline = 'middle'

  const titleCenterY = coverY + coverSize + 60 + (titleLines * titleLineH) / 2
  ctx.fillStyle = onBgColor
  ctx.font = `bold ${titleFont}px ${config.fontFamily}`
  wrapText(ctx, title, width / 2, titleCenterY, contentWidth, titleLineH)

  const artistCenterY =
    titleCenterY + (titleLines * titleLineH) / 2 + titleGap + (artistLines * artistLineH) / 2
  ctx.fillStyle = onSurfaceVariant
  ctx.font = `${artistFont}px ${config.fontFamily}`
  wrapText(ctx, artist, width / 2, artistCenterY, contentWidth, artistLineH)

  // ==== 绘制歌词（在信息区与进度条之间垂直居中，字号自适应） ====
  if (hasLyrics && lyricMeasure) {
    const lyricsAreaTop = artistCenterY + (artistLines * artistLineH) / 2 + 40
    const lyricsAreaBottom = height - padding - 140
    const lyricsAreaCenterY = (lyricsAreaTop + lyricsAreaBottom) / 2

    const m = lyricMeasure
    const mainBlockH = m.mainLines * m.mainLineH

    const mainCenterY = lyricsAreaCenterY - m.height / 2 + mainBlockH / 2
    ctx.fillStyle = primaryColor
    ctx.font = `bold ${m.mainFont}px ${config.fontFamily}`
    wrapText(ctx, mainText, width / 2, mainCenterY, contentWidth, m.mainLineH)

    if (transText && m.transLines > 0) {
      const transCenterY =
        mainCenterY + mainBlockH / 2 + m.gap + (m.transLines * m.transLineH) / 2
      ctx.save()
      ctx.fillStyle = primaryColor
      ctx.globalAlpha = 0.85
      ctx.font = `${m.transFont}px ${config.fontFamily}`
      wrapText(ctx, transText, width / 2, transCenterY, contentWidth, m.transLineH)
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
 * 自适应排版：空间富余时放大封面/标题/歌词填满画布，
 * 空间不足时缩小歌词字号防止溢出（极端情况加高画布）
 */
const generateCompactImage = async (options = {}) => {
  const config = { ...getConfig(), ...options }
  const { width, padding } = config
  const maxLyricWidth = width - padding * 2

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
  const hasLyrics = mainText.length > 0

  await yieldToMain()

  // ==== 自适应排版：先测量，定稿尺寸后再绘制 ====
  const tempCanvas = api.utils.createCanvas(width, 100)
  const tempCtx = tempCanvas.ctx

  const title = state.currentTrack.title || state.currentTrack.name || '未知歌曲'
  const artist = state.currentTrack.artist || '未知艺术家'

  // 基准尺寸
  const baseCoverSize = 200
  const topGap = 40 // 顶部水印与歌词区的间距

  // 基准歌词块高度（无歌词时按 200px 预留）
  const baseLyricH = hasLyrics
    ? measureLyricBlock(tempCtx, mainText, transText, maxLyricWidth, 1, config.fontFamily).height
    : 200

  // 底部区域高度 = 底部留白 + 封面（左侧信息与封面对齐）
  const baseBottomH = padding + baseCoverSize

  // 自然内容高度：顶部留白 + 歌词 + 间距 + 底部区域
  const naturalHeight = padding + topGap + baseLyricH + 20 + baseBottomH

  // 画布高度：紧凑布局 800 ~ 1600
  let height = Math.max(800, Math.min(1600, naturalHeight))

  // ---- 分配富余空间：放大封面（底部区域随之变高），剩余空间留给歌词 ----
  const extra = height - naturalHeight
  let coverSize = baseCoverSize
  if (extra > 0) {
    // 无歌词时封面多分一些，避免中部大面积留白
    const ratio = hasLyrics ? 0.35 : 0.6
    coverSize = baseCoverSize + Math.min(extra * ratio, 160)
  }
  const bottomH = padding + coverSize

  // ---- 歌词区域：按实际剩余空间自适应缩放 ----
  const lyricAreaTop = padding + topGap
  const lyricAreaBottom = height - 20 - bottomH

  let lyricMeasure = null
  if (hasLyrics) {
    lyricMeasure = fitLyricScale(
      tempCtx,
      mainText,
      transText,
      maxLyricWidth,
      lyricAreaBottom - lyricAreaTop,
      config.fontFamily,
    )
    if (!lyricMeasure) {
      // 最小字号也放不下：加高画布承接全部歌词，避免文字溢出
      lyricMeasure = measureLyricBlock(
        tempCtx,
        mainText,
        transText,
        maxLyricWidth,
        0.65,
        config.fontFamily,
      )
      height = lyricAreaTop + lyricMeasure.height + 20 + bottomH
    }
  }

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

  // ========== 绘制歌词区域（上方，垂直居中，字号自适应） ==========
  const lyricsAreaBottom = height - 20 - bottomH
  const lyricsAreaCenterY = (lyricAreaTop + lyricsAreaBottom) / 2

  if (hasLyrics && lyricMeasure) {
    const m = lyricMeasure
    const mainBlockH = m.mainLines * m.mainLineH

    const mainCenterY = lyricsAreaCenterY - m.height / 2 + mainBlockH / 2
    ctx.fillStyle = primaryColor
    ctx.font = `bold ${m.mainFont}px ${config.fontFamily}`
    ctx.textAlign = 'center'
    ctx.textBaseline = 'middle'
    wrapText(ctx, mainText, width / 2, mainCenterY, maxLyricWidth, m.mainLineH)

    // 绘制翻译歌词
    if (transText && m.transLines > 0) {
      const transCenterY =
        mainCenterY + mainBlockH / 2 + m.gap + (m.transLines * m.transLineH) / 2
      ctx.save()
      ctx.fillStyle = primaryColor
      ctx.globalAlpha = 0.85
      ctx.font = `${m.transFont}px ${config.fontFamily}`
      wrapText(ctx, transText, width / 2, transCenterY, maxLyricWidth, m.transLineH)
      ctx.restore()
    }
  }

  // ========== 底部区域 ==========
  // 布局：
  // - 封面：右下角，尺寸随富余空间放大
  // - 左侧从上到下：进度条 -> 时间 -> 标题 -> 艺术家（字号随封面小幅放大）

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

  // 标题/艺人字号随封面放大（一半力度，避免挤压左侧宽度）
  const fontScale = Math.min(1.3, 1 + (coverSize - baseCoverSize) / baseCoverSize / 2)

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
    ctx.font = `${Math.round(18 * fontScale)}px ${config.fontFamily}`
    ctx.textAlign = 'left'
    ctx.textBaseline = 'top'
    ctx.fillText(api.utils.formatTime(state.currentTime), progressX, progressY + 14)
    ctx.textAlign = 'right'
    ctx.fillText(api.utils.formatTime(state.duration), progressX + progressWidth, progressY + 14)
  }

  // 歌曲标题（在底部区域的中下部）
  const titleY = leftAreaBottom - Math.round(60 * fontScale)
  ctx.fillStyle = onBgColor
  ctx.font = `bold ${Math.round(40 * fontScale)}px ${config.fontFamily}`
  ctx.textAlign = 'left'
  ctx.textBaseline = 'bottom'
  const titleTruncated = truncateText(ctx, title, infoMaxWidth)
  ctx.fillText(titleTruncated, infoX, titleY)

  // 艺术家（在标题下方）
  ctx.fillStyle = onSurfaceVariant
  ctx.font = `${Math.round(28 * fontScale)}px ${config.fontFamily}`
  ctx.textBaseline = 'top'
  const artistTruncated = truncateText(ctx, artist, infoMaxWidth)
  ctx.fillText(artistTruncated, infoX, titleY + Math.round(16 * fontScale))

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
