/**
 * 歌词截图分享插件
 * 生成精美的歌词分享图片，支持保存和复制到剪贴板
 */

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
}

/**
 * 获取配置
 */
const getConfig = () => {
  return { ...defaultConfig, ...api.storage.get('config', {}) }
}

/**
 * 绘制圆角矩形
 */
const roundRect = (ctx, x, y, w, h, r) => {
  ctx.beginPath()
  ctx.moveTo(x + r, y)
  ctx.lineTo(x + w - r, y)
  ctx.quadraticCurveTo(x + w, y, x + w, y + r)
  ctx.lineTo(x + w, y + h - r)
  ctx.quadraticCurveTo(x + w, y + h, x + w - r, y + h)
  ctx.lineTo(x + r, y + h)
  ctx.quadraticCurveTo(x, y + h, x, y + h - r)
  ctx.lineTo(x, y + r)
  ctx.quadraticCurveTo(x, y, x + r, y)
  ctx.closePath()
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

  return { totalHeight, lines: lines.length, startY, endY: startY + (lines.length - 1) * lineHeight }
}

// 让出主线程的辅助函数
const yieldToMain = () => new Promise(resolve => setTimeout(resolve, 0))

/**
 * 生成分享图片
 */
const generateImage = async (options = {}) => {
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

  // 获取当前歌词
  let currentLyric = null
  let mainText = ''
  let transText = ''
  
  if (lyrics && lyrics.length > 0) {
    if (lyricIndex >= 0 && lyricIndex < lyrics.length) {
      currentLyric = lyrics[lyricIndex]
    } else {
      currentLyric = lyrics[0]
    }
    if (currentLyric) {
      mainText = currentLyric.texts?.[0] || currentLyric.text || ''
      transText = currentLyric.texts?.[1] || currentLyric.translation || ''
    }
  }

  // 让出主线程，避免长时间阻塞
  await yieldToMain()

  // 计算歌词需要的行数来决定高度
  const tempCanvas = api.utils.createCanvas(width, 100)
  const tempCtx = tempCanvas.ctx
  const maxLyricWidth = width - padding * 2
  
  // 计算主歌词行数
  tempCtx.font = `bold 56px ${config.fontFamily}`
  let mainLines = 0
  if (mainText) {
    mainLines = 1
    let line = ''
    for (const char of mainText) {
      const testLine = line + char
      if (tempCtx.measureText(testLine).width > maxLyricWidth && line.length > 0) {
        mainLines++
        line = char
      } else {
        line = testLine
      }
    }
  }
  
  // 计算翻译行数
  tempCtx.font = `52px ${config.fontFamily}`
  let transLines = 0
  if (transText) {
    transLines = 1
    let line = ''
    for (const char of transText) {
      const testLine = line + char
      if (tempCtx.measureText(testLine).width > maxLyricWidth && line.length > 0) {
        transLines++
        line = char
      } else {
        line = testLine
      }
    }
  }

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

  // 直接获取 CSS 变量值，确保颜色一致
  const bgColor = api.theme.getCSSVariable('md-sys-color-background') || (isDark ? '#121212' : '#fefefe')
  let primaryColor = api.theme.getCSSVariable('md-sys-color-primary') || '#6750a4'
  const onBgColor = api.theme.getCSSVariable('md-sys-color-on-background') || (isDark ? '#e6e1e5' : '#1c1b1f')
  const onSurfaceVariant = api.theme.getCSSVariable('md-sys-color-on-surface-variant') || (isDark ? '#cac4d0' : '#49454f')
  const surfaceContainer = api.theme.getCSSVariable('md-sys-color-surface-container') || (isDark ? '#211f26' : '#f3edf7')

  // 绘制纯色背景
  ctx.fillStyle = bgColor
  ctx.fillRect(0, 0, width, height)

  // 让出主线程
  await yieldToMain()

  // 预加载封面图片（只加载一次）
  let coverImg = null
  if (state.currentTrack.cover) {
    try {
      coverImg = await api.utils.loadImage(state.currentTrack.cover)
    } catch (e) {
      api.log.debug('封面加载失败:', e)
    }
  }

  // 绘制封面背景（模糊效果）
  if (config.showCover && coverImg) {
    // 绘制模糊背景
    ctx.save()
    ctx.globalAlpha = config.coverOpacity
    ctx.filter = `blur(${config.coverBlur}px)`
    
    // 计算覆盖整个画布的尺寸
    const scale = Math.max(width / coverImg.width, height / coverImg.height) * 1.2
    const scaledW = coverImg.width * scale
    const scaledH = coverImg.height * scale
    const offsetX = (width - scaledW) / 2
    const offsetY = (height - scaledH) / 2
    
    ctx.drawImage(coverImg, offsetX, offsetY, scaledW, scaledH)
    ctx.restore()

    // 添加渐变遮罩
    const gradient = ctx.createLinearGradient(0, 0, 0, height)
    gradient.addColorStop(0, isDark ? 'rgba(18,18,18,0.7)' : 'rgba(254,254,254,0.7)')
    gradient.addColorStop(0.5, isDark ? 'rgba(18,18,18,0.5)' : 'rgba(254,254,254,0.5)')
    gradient.addColorStop(1, isDark ? 'rgba(18,18,18,0.9)' : 'rgba(254,254,254,0.9)')
    ctx.fillStyle = gradient
    ctx.fillRect(0, 0, width, height)
  }

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
  const lyricsAreaTop = artistY + artistInfo.totalHeight / 2 + 40  // 艺术家下方
  const lyricsAreaBottom = height - padding - 140  // 进度条上方
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

// 防抖标志，防止重复执行导致页面冻结
let isGenerating = false

// 插件主体
const plugin = {
  activate() {
    api.log.info('歌词截图分享插件已激活')
    
    // 注册快捷键
    api.shortcuts.register({
      id: 'lyrics-share-copy',
      name: '复制歌词图片',
      key: 'Ctrl+Shift+C',
      description: '将当前歌词生成图片并复制到剪贴板',
      action: () => this.copyImage()
    })
    
    api.shortcuts.register({
      id: 'lyrics-share-save',
      name: '保存歌词图片',
      key: 'Ctrl+Shift+S',
      description: '将当前歌词生成图片并保存到本地',
      action: () => this.saveImage()
    })
    
    // 注册操作按钮
    api.ui.registerActionButton({
      id: 'lyrics-share-copy-btn',
      name: '复制歌词图片 (Ctrl+Shift+C)',
      icon: 'content_copy',
      location: 'lyrics',
      action: () => this.copyImage()
    })
    
    api.ui.registerActionButton({
      id: 'lyrics-share-save-btn',
      name: '保存歌词图片 (Ctrl+Shift+S)',
      icon: 'save',
      location: 'lyrics',
      action: () => this.saveImage()
    })
  },

  deactivate() {
    // 取消注册快捷键
    api.shortcuts.unregister('lyrics-share-copy')
    api.shortcuts.unregister('lyrics-share-save')
    // 取消注册按钮
    api.ui.unregisterActionButton('lyrics-share-copy-btn')
    api.ui.unregisterActionButton('lyrics-share-save-btn')
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
      await new Promise(resolve => setTimeout(resolve, 10))
      
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
      await new Promise(resolve => setTimeout(resolve, 10))
      
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
}
