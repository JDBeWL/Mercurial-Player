import { ref, watch, type Ref } from 'vue'
import { readFile } from '@tauri-apps/plugin-fs'

/**
 * 从封面图片提取主色，用于沉浸式封面模式的背景填充。
 *
 * 核心流程：
 * 1. 降采样解码：读取本地或网络图片数据，绘制至 32x32 Canvas 提取像素
 * 2. 种子选定：基于直方图选出高频且相互有一定色差的 OKLab 种子点
 * 3. K-Means 聚类：在感知均匀的 OKLab 空间中迭代聚类像素
 * 4. 综合打分：结合色块占比与彩度选取合适的主色（抑制过暗与高光点）
 * 5. 亮度钳制与色域保护：将主色压暗至适合作为暗色 UI 背景的区间，同时按比例缩放彩度避免 RGB 溢出偏色
 */
export function useDominantColor(coverPath: Ref<string | undefined | null>) {
  /** 提取出的主色（rgb 字符串），空串表示未提取或提取失败 */
  const dominantColor = ref('')

  // 代际计数：用于切换封面时丢弃已过期的异步提取任务
  let generation = 0

  // ===== OKLab 色彩空间转换（Björn Ottosson 算法） =====

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

  const oklabToRgb = (L: number, a: number, b: number) => {
    const l_ = L + 0.3963377774 * a + 0.2158037573 * b
    const m_ = L - 0.1055613458 * a - 0.0638541728 * b
    const s_ = L - 0.0894841775 * a - 1.291485548 * b
    const l = l_ * l_ * l_
    const m = m_ * m_ * m_
    const s = s_ * s_ * s_
    return {
      r: linearToSrgb(4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s),
      g: linearToSrgb(-1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s),
      b: linearToSrgb(-0.0041960863 * l - 0.7034186147 * m + 1.707614701 * s),
    }
  }

  /** 根据文件路径后缀推断 MIME 类型 */
  const mimeFromPath = (path: string): string => {
    const ext = path.split('.').pop()?.toLowerCase() ?? ''
    if (ext === 'png') return 'image/png'
    if (ext === 'webp') return 'image/webp'
    return 'image/jpeg'
  }

  /**
   * 读取图片并降采样为 32x32 像素数组
   * 兼容本地文件路径（FS API）与网络/Protocol URL（Fetch API）
   */
  const decodePixels = async (path: string): Promise<Uint8ClampedArray | null> => {
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
    const ctx = canvas.getContext('2d', { willReadFrequently: true })
    if (!ctx) return null

    if (typeof createImageBitmap === 'function') {
      const bitmap = await createImageBitmap(blob, {
        resizeWidth: SIZE,
        resizeHeight: SIZE,
        resizeQuality: 'low',
      })
      ctx.drawImage(bitmap, 0, 0, SIZE, SIZE)
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
        ctx.drawImage(img, 0, 0, SIZE, SIZE)
      } finally {
        URL.revokeObjectURL(url)
      }
    }

    return ctx.getImageData(0, 0, SIZE, SIZE).data
  }

  /**
   * 从像素数组中计算并提炼出适合作为 UI 背景的主色
   */
  const pickDominant = (pixels: Uint8ClampedArray): string => {
    // 1. 转入 OKLab 空间，同时使用 4bit RGB 桶聚合像素，为初始种子收集数据
    const pts: { L: number; a: number; b: number }[] = []
    const buckets = new Map<number, { count: number; L: number; a: number; b: number }>()

    for (let i = 0; i < pixels.length; i += 4) {
      if (pixels[i + 3] < 128) continue // 忽略半透明像素
      const { L, a, b } = rgbToOklab(pixels[i], pixels[i + 1], pixels[i + 2])
      pts.push({ L, a, b })

      const key = ((pixels[i] >> 4) << 8) | ((pixels[i + 1] >> 4) << 4) | (pixels[i + 2] >> 4)
      let agg = buckets.get(key)
      if (!agg) buckets.set(key, (agg = { count: 0, L: 0, a: 0, b: 0 }))
      agg.count++
      agg.L += L
      agg.a += a
      agg.b += b
    }

    if (pts.length === 0) return ''

    // 2. 种子筛选：按出现频率排序，贪心挑选彼此色差（OKLab 欧氏距离）大于阈值的种子
    const sorted = [...buckets.values()].sort((x, y) => y.count - x.count)
    const seeds: { L: number; a: number; b: number }[] = []
    const MIN_SEED_DIST = 0.12

    for (const agg of sorted) {
      if (seeds.length >= 6) break
      const c = { L: agg.L / agg.count, a: agg.a / agg.count, b: agg.b / agg.count }
      const farEnough = seeds.every(
        (s) => Math.hypot(c.L - s.L, c.a - s.a, c.b - s.b) >= MIN_SEED_DIST,
      )
      if (farEnough) seeds.push(c)
    }

    // 3. K-Means 聚类：在 OKLab 空间迭代计算近邻中心
    let centers = seeds.map((s) => [s.L, s.a, s.b] as [number, number, number])
    const assign = new Int32Array(pts.length)

    for (let iter = 0; iter < 10; iter++) {
      for (let p = 0; p < pts.length; p++) {
        let best = 0
        let bestD = Infinity
        for (let c = 0; c < centers.length; c++) {
          const dL = pts[p].L - centers[c][0]
          const da = pts[p].a - centers[c][1]
          const db = pts[p].b - centers[c][2]
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
        const s = sums[assign[p]]
        s.n++
        s.L += pts[p].L
        s.a += pts[p].a
        s.b += pts[p].b
      }
      centers = sums.map((s, i) =>
        s.n > 0 ? ([s.L / s.n, s.a / s.n, s.b / s.n] as [number, number, number]) : centers[i],
      )
    }

    // 4. 打分：根据色彩占比与彩度综合计算，抑制极暗与极亮像素
    const counts = new Array<number>(centers.length).fill(0)
    for (let p = 0; p < pts.length; p++) counts[assign[p]]++

    let best = { score: -1, L: 0, a: 0, b: 0 }
    for (let c = 0; c < centers.length; c++) {
      const [L, a, b] = centers[c]
      const share = counts[c] / pts.length
      const chroma = Math.hypot(a, b)
      let score = Math.sqrt(share) * (0.15 + chroma * 2)

      if (L < 0.15) score *= 0.25 // 降权暗色（多为边框或阴影）
      if (L > 0.9) score *= 0.5   // 降权过亮高光
      if (score > best.score) best = { score, L, a, b }
    }

    // 5. 沉浸式 UI 适配：限制最大亮度，并依比例衰减彩度以防止 sRGB 色域溢出
    const targetL = Math.min(best.L, 0.55)
    const dimRatio = best.L > 0 ? Math.min(1, targetL / best.L) : 1

    let finalA = best.a * dimRatio
    let finalB = best.b * dimRatio

    // 钳制最高彩度，避免背景色过于刺眼
    const currentChroma = Math.hypot(finalA, finalB)
    if (currentChroma > 0.18) {
      const k = 0.18 / currentChroma
      finalA *= k
      finalB *= k
    }

    const { r, g, b } = oklabToRgb(targetL, finalA, finalB)
    return `rgb(${r}, ${g}, ${b})`
  }

  watch(
    coverPath,
    (path) => {
      const gen = ++generation
      if (!path) {
        dominantColor.value = ''
        return
      }

      decodePixels(path)
        .then((pixels) => {
          if (gen === generation) {
            dominantColor.value = pixels ? pickDominant(pixels) : ''
          }
        })
        .catch(() => {
          if (gen === generation) {
            dominantColor.value = ''
          }
        })
    },
    { immediate: true },
  )

  return { dominantColor }
}