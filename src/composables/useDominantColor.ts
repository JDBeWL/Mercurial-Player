import { ref, watch, type Ref } from 'vue'
import { readFile } from '@tauri-apps/plugin-fs'
import type { ImmersiveColorScheme } from '@/types'
import errorHandler, { ErrorSeverity } from '@/utils/errorHandler'

/**
 * 从封面提取主色,用于沉浸式背景填充;取样区域由 decodePixels 按 .full-cover 规则裁出
 *
 * - 'album': 整张均匀取样 + K-Means 选代表性色 (偏中亮度高彩度,抑制过暗/过亮与灰)
 * - 'fusion': 只取最右 5% 羽化条带求平均,与封面右缘一致,过渡最无痕
 */
export function useDominantColor(
  coverPath: Ref<string | undefined | null>,
  mode: Ref<ImmersiveColorScheme> = ref('album'),
) {
  const dominantColor = ref('')
  // 主色的 OKLab 亮度 (0~1),取色失败时为 null;沉浸式据此切换深/浅主题,保证前景文字可读
  const dominantLuminance = ref<number | null>(null)
  let generation = 0

  // OKLab 转换,下面的矩阵常量取自 Björn Ottosson 的参考实现
  const srgbToLinear = (c: number): number => {
    const v = c / 255
    return v <= 0.04045 ? v / 12.92 : Math.pow((v + 0.055) / 1.055, 2.4)
  }

  const linearToSrgb = (c: number): number => {
    const v = c <= 0.0031308 ? 12.92 * c : 1.055 * Math.pow(c, 1 / 2.4) - 0.055
    return Math.round(Math.min(1, Math.max(0, v)) * 255)
  }

  const rgbToOklab = (r: number, g: number, b: number) => {
    const lr = srgbToLinear(r)
    const lg = srgbToLinear(g)
    const lb = srgbToLinear(b)
    const l = Math.cbrt(0.4122214708 * lr + 0.5363325363 * lg + 0.0514459929 * lb)
    const m = Math.cbrt(0.2119034982 * lr + 0.6806995451 * lg + 0.1073969566 * lb)
    const s = Math.cbrt(0.0883024619 * lr + 0.2817188376 * lg + 0.6299787005 * lb)
    return {
      L: 0.2104542553 * l + 0.793617785 * m - 0.0040720468 * s,
      a: 1.9779984951 * l - 2.428592205 * m + 0.4505937099 * s,
      b: 0.0259040371 * l + 0.7827717662 * m - 0.808675766 * s,
    }
  }

  // 返回 0~1 浮点 (未取整),色域映射时要靠越界的小数部分判断
  const oklabToRgbFloat = (L: number, a: number, b: number) => {
    const l_ = L + 0.3963377774 * a + 0.2158037573 * b
    const m_ = L - 0.1055613458 * a - 0.0638541728 * b
    const s_ = L - 0.0894841775 * a - 1.291485548 * b
    const l = l_ * l_ * l_
    const m = m_ * m_ * m_
    const s = s_ * s_ * s_
    return {
      r: 4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s,
      g: -1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s,
      b: -0.0041960863 * l - 0.7034186147 * m + 1.707614701 * s,
    }
  }

  // 返回取整后的 0~255 整数,用于最终输出
  const oklabToRgb = (L: number, a: number, b: number) => {
    const { r, g, b: blue } = oklabToRgbFloat(L, a, b)
    return {
      r: linearToSrgb(r),
      g: linearToSrgb(g),
      b: linearToSrgb(blue),
    }
  }

  const mimeFromPath = (path: string): string => {
    const ext = path.split('.').pop()?.toLowerCase() ?? ''
    if (ext === 'png') return 'image/png'
    if (ext === 'webp') return 'image/webp'
    return 'image/jpeg'
  }

  /** 读图片并降采样为 32x32 像素数组:取色只需低频统计,固定小尺寸让算法代价与原图分辨率无关 */
  const decodePixels = async (path: string): Promise<Uint8ClampedArray | null> => {
    // 远程与本地路径都先拿到 Blob 再解码:把跨域 URL 直接画进 canvas 会污染画布,getImageData 就抛错
    let blob: Blob
    if (
      path.startsWith('http://') ||
      path.startsWith('https://') ||
      path.startsWith('data:') ||
      path.startsWith('asset://')
    ) {
      const res = await fetch(path)
      blob = await res.blob()
    } else {
      const data = await readFile(path)
      blob = new Blob([data], { type: mimeFromPath(path) })
    }

    const SIZE = 32
    const canvas = document.createElement('canvas')
    canvas.width = SIZE
    canvas.height = SIZE
    // getImageData 是热路径,willReadFrequently 让浏览器留在 CPU 后端而非 GPU 回读
    const ctx = canvas.getContext('2d', { willReadFrequently: true })
    if (!ctx) return null

    // 与显示端 CSS (object-fit: cover + object-position: left center) 一致:等比铺满 SIZE 后左对齐、
    // 垂直居中,画布外自然裁掉,这样非方形图取样到的就是用户实际看到的区域
    const drawCoverCropped = (source: CanvasImageSource, sw: number, sh: number) => {
      const scale = Math.max(SIZE / sw, SIZE / sh)
      const dw = sw * scale
      const dh = sh * scale
      ctx.drawImage(source, 0, (SIZE - dh) / 2, dw, dh)
    }

    if (typeof createImageBitmap === 'function') {
      const bitmap = await createImageBitmap(blob)
      drawCoverCropped(bitmap, bitmap.width, bitmap.height)
      bitmap.close()
    } else {
      const url = URL.createObjectURL(blob)
      try {
        const img = await new Promise<HTMLImageElement>((resolve, reject) => {
          const image = new Image()
          image.onload = () => resolve(image)
          image.onerror = () => reject(new Error('image decode failed'))
          image.src = url
        })
        drawCoverCropped(img, img.naturalWidth, img.naturalHeight)
      } finally {
        URL.revokeObjectURL(url)
      }
    }

    return ctx.getImageData(0, 0, SIZE, SIZE).data
  }

  /** 亮度上限压缩 + 彩度等比缩放 + 色域映射,输出最终 rgb() 字符串与亮度 (两种模式共用) */
  const finalizeColor = (L: number, a: number, b: number): { color: string; luminance: number } => {
    // 亮度封顶 0.95:再压低主色会偏暗,盖在上面的歌词反而看不清
    const targetL = Math.min(L, 0.95)

    // 彩度按亮度压缩比例等比缩放,否则压暗后仍保持原彩度会偏色
    const dimRatio = targetL / Math.max(L, 0.001)
    let finalA = a * dimRatio
    let finalB = b * dimRatio

    // 色域映射:固定色相二分找出彩度的最大合法值,保证 RGB 落在 [0,1]
    if (Math.hypot(finalA, finalB) > 0) {
      let lo = 0
      let hi = 1
      for (let i = 0; i < 25; i++) {
        const mid = (lo + hi) / 2
        const { r, g, b: blue } = oklabToRgbFloat(targetL, finalA * mid, finalB * mid)
        if (
          r >= -1e-6 &&
          r <= 1 + 1e-6 &&
          g >= -1e-6 &&
          g <= 1 + 1e-6 &&
          blue >= -1e-6 &&
          blue <= 1 + 1e-6
        ) {
          lo = mid
        } else {
          hi = mid
        }
      }
      finalA *= lo
      finalB *= lo
    }

    const { r, g, b: blue } = oklabToRgb(targetL, finalA, finalB)
    return { color: `rgb(${r}, ${g}, ${blue})`, luminance: targetL }
  }

  const pickDominant = (
    pixels: Uint8ClampedArray,
    mode: ImmersiveColorScheme,
  ): { color: string; luminance: number } => {
    // 不透明像素转入 OKLab;fusion 只留最右 5% 的羽化条带 (两种模式的定义见文件头)
    const width = Math.round(Math.sqrt(pixels.length / 4))
    const pts: { L: number; a: number; b: number }[] = []
    for (let p = 0; p * 4 < pixels.length; p++) {
      const i = p * 4
      if (pixels[i + 3]! < 128) continue
      const x = width > 1 ? (p % width) / (width - 1) : 1
      if (mode === 'fusion' && x < 0.95) continue
      const { L, a, b } = rgbToOklab(pixels[i]!, pixels[i + 1]!, pixels[i + 2]!)
      pts.push({ L, a, b })
    }

    if (pts.length === 0) {
      // 无有效像素 => 深灰后备色 (OKLab L ~= 0.19)
      return { color: 'rgb(40, 40, 40)', luminance: 0.19 }
    }

    // fusion 取条带平均而非 K-Means:背景要贴近右缘真实观感,双色条带下主色会造成突兀过渡
    if (mode === 'fusion') {
      let sumL = 0,
        suma = 0,
        sumb = 0
      for (const p of pts) {
        sumL += p.L
        suma += p.a
        sumb += p.b
      }
      return finalizeColor(sumL / pts.length, suma / pts.length, sumb / pts.length)
    }

    // 有效采样点少于 20 个:样本太少不足以聚类,直接取平均色,彩度减半避免浑浊
    if (pts.length < 20) {
      let sumL = 0,
        suma = 0,
        sumb = 0
      for (const p of pts) {
        sumL += p.L
        suma += p.a
        sumb += p.b
      }
      return finalizeColor(sumL / pts.length, (suma / pts.length) * 0.5, (sumb / pts.length) * 0.5)
    }

    // K-means++ 初始化种子,最多 6 个簇
    const k = Math.min(6, pts.length)
    const seeds: typeof pts = []
    const firstIdx = Math.floor(Math.random() * pts.length)
    seeds.push({ ...pts[firstIdx]! })

    for (let i = 1; i < k; i++) {
      const distSq = pts.map((p) => {
        let minD = Infinity
        for (const s of seeds) {
          const d = Math.hypot(p.L - s.L, p.a - s.a, p.b - s.b)
          if (d < minD) minD = d
        }
        return minD * minD
      })
      const total = distSq.reduce((a, b) => a + b, 0)
      if (total === 0) break // 所有点都已成为种子,理论上到不了这里
      let r = Math.random() * total
      for (let j = 0; j < distSq.length; j++) {
        r -= distSq[j]!
        if (r <= 0) {
          seeds.push({ ...pts[j]! })
          break
        }
      }
    }

    // K-Means 主迭代,最多 10 轮,通常由下面的收敛判断提前 break
    let centers = seeds.map((s) => [s.L, s.a, s.b] as [number, number, number])
    const assign = new Int32Array(pts.length)

    for (let iter = 0; iter < 10; iter++) {
      for (let p = 0; p < pts.length; p++) {
        let best = 0
        let bestD = Infinity
        for (let c = 0; c < centers.length; c++) {
          const dL = pts[p]!.L - centers[c]![0]
          const da = pts[p]!.a - centers[c]![1]
          const db = pts[p]!.b - centers[c]![2]
          const d = dL * dL + da * da + db * db
          if (d < bestD) {
            bestD = d
            best = c
          }
        }
        assign[p] = best
      }

      const sums = centers.map(() => ({ n: 0, L: 0, a: 0, b: 0 }))
      for (let p = 0; p < pts.length; p++) {
        const s = sums[assign[p]!]!
        s.n++
        s.L += pts[p]!.L
        s.a += pts[p]!.a
        s.b += pts[p]!.b
      }

      let maxMove = 0
      const newCenters = sums.map((s, i) => {
        if (s.n === 0) return centers[i]!
        const nc: [number, number, number] = [s.L / s.n, s.a / s.n, s.b / s.n]
        const move = Math.hypot(
          nc[0] - centers[i]![0],
          nc[1] - centers[i]![1],
          nc[2] - centers[i]![2],
        )
        if (move > maxMove) maxMove = move
        return nc
      })
      centers = newCenters
      if (maxMove < 1e-5) break // 簇中心位移小于 1e-5 (OKLab 单位) 视为收敛
    }

    const counts = new Array<number>(centers.length).fill(0)
    for (let p = 0; p < pts.length; p++) counts[assign[p]!]!++

    // 综合打分挑代表色
    let best = { score: -1, L: 0, a: 0, b: 0 }
    for (let c = 0; c < centers.length; c++) {
      const [L, a, b] = centers[c]!
      const share = counts[c]! / pts.length
      const chroma = Math.hypot(a, b)

      // 基础分:占比重、彩度高
      let score = Math.sqrt(share) * (0.2 + chroma * 2.5)

      // 偏好中等亮度 (适合深色背景),过暗过亮都降权
      if (L >= 0.25 && L <= 0.45) score *= 1.3
      else if (L < 0.2 || L > 0.65) score *= 0.6

      // 抑制接近灰的簇
      if (chroma < 0.02) score *= 0.3

      if (score > best.score) {
        best = { score, L, a, b }
      }
    }

    return finalizeColor(best.L, best.a, best.b)
  }

  watch(
    [coverPath, mode],
    ([path, currentMode]) => {
      // 代次守卫:每次封面/模式变化都递增,过期的异步回调不许再写入结果
      const gen = ++generation
      if (!path) {
        dominantColor.value = ''
        dominantLuminance.value = null
        return
      }

      decodePixels(path)
        .then((pixels) => {
          if (gen === generation) {
            if (pixels) {
              const result = pickDominant(pixels, currentMode)
              dominantColor.value = result.color
              dominantLuminance.value = result.luminance
            } else {
              dominantColor.value = ''
              dominantLuminance.value = null
            }
          }
        })
        .catch((e) => {
          // 取色失败:回退为无主色,沉浸层改用主题后备色
          errorHandler.handle(e, { severity: ErrorSeverity.LOW, showToUser: false })
          if (gen === generation) {
            dominantColor.value = ''
            dominantLuminance.value = null
          }
        })
    },
    { immediate: true },
  )

  return { dominantColor, dominantLuminance }
}
