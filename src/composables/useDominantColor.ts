import { ref, watch, type Ref } from 'vue'
import { readFile } from '@tauri-apps/plugin-fs'
import type { ImmersiveColorScheme } from '@/types'

/**
 * 从封面图片提取主色，用于沉浸式封面模式的背景填充。
 *
 * 两种取色风格（mode 参数）：
 * - 'album' 专辑主题色：整张封面均匀取样，K‑Means 选出最具代表性且美观的主题色。
 * - 'fusion' 封面融合：只取封面最右侧约 20% 的羽化条带做加权平均，
 *   背景色与封面右缘一致，封面"沉入"背景的过渡最无痕（对部分封面视觉最佳）。
 *
 * 通用优化点：
 * 1. 种子初始化：采用 K‑means++ 从所有像素中均匀选取初始簇心，替代粗糙的 4bit 桶聚合。
 * 2. K‑Means 迭代：增加收敛判断，提前终止以减少无效计算。
 * 3. 打分策略（album 模式）：更偏重中等亮度（L 0.25~0.45）和高彩度，抑制过暗/过亮及灰色。
 * 4. 色域映射：在 OKLab 空间保持色相，通过二分查找最大合法彩度，避免 RGB 溢出。
 * 5. 鲁棒性：处理纯色、透明过多等极端情况，返回安全后备色。
 * 6. 取样区域与显示一致：按 object-fit: cover + object-position: left center
 *    裁剪后再取样，非方形图取到的就是实际显示的区域。
 */
export function useDominantColor(
  coverPath: Ref<string | undefined | null>,
  mode: Ref<ImmersiveColorScheme> = ref('album')
) {
  const dominantColor = ref('')
  // 主色的 OKLab 亮度（0~1），取色失败时为 null。
  // 沉浸式模式据此自动切换应用的深/浅主题，保证前景文字可读。
  const dominantLuminance = ref<number | null>(null)
  let generation = 0

  // ===================== 色彩空间转换（Björn Ottosson） =====================

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

  // 返回 0-1 浮点数（未取整），用于色域映射时的精确判断
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

  // 返回取整后的 0-255 整数（用于最终输出）
  const oklabToRgb = (L: number, a: number, b: number) => {
    const { r, g, b: blue } = oklabToRgbFloat(L, a, b)
    return {
      r: linearToSrgb(r),
      g: linearToSrgb(g),
      b: linearToSrgb(blue),
    }
  }

  // ===================== 辅助函数 =====================

  const mimeFromPath = (path: string): string => {
    const ext = path.split('.').pop()?.toLowerCase() ?? ''
    if (ext === 'png') return 'image/png'
    if (ext === 'webp') return 'image/webp'
    return 'image/jpeg'
  }

  /**
   * 读取图片并降采样为 32x32 像素数组
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

    // 与显示端 CSS（object-fit: cover + object-position: left center）保持一致：
    // 等比缩放铺满 SIZE 后左对齐、垂直居中，画布外区域自然被裁掉。
    // 这样非方形图取样到的就是用户实际看到的裁剪区域。
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

  // ===================== 核心取色算法 =====================

  /** 亮度上限压缩 + 彩度等比缩放 + 色域映射，输出最终 rgb() 字符串与亮度（两种模式共用） */
  const finalizeColor = (L: number, a: number, b: number): { color: string; luminance: number } => {
    // 亮度压缩（为了能看清歌词视觉上最好不超过 0.65，保证可读，现在暂时调高一点我感觉观感更好）
    // const targetL = Math.min(L, 0.65)
    const targetL = Math.min(L, 0.95)

    // 按亮度压缩比例缩放彩度，防止压缩后彩度不变导致偏色
    const dimRatio = targetL / Math.max(L, 0.001)
    let finalA = a * dimRatio
    let finalB = b * dimRatio

    // 色域映射：保持色相，二分查找最大合法彩度，保证 RGB 在 [0,1] 内
    if (Math.hypot(finalA, finalB) > 0) {
      let lo = 0
      let hi = 1
      for (let i = 0; i < 25; i++) {
        const mid = (lo + hi) / 2
        const { r, g, b: blue } = oklabToRgbFloat(targetL, finalA * mid, finalB * mid)
        if (
          r >= -1e-6 && r <= 1 + 1e-6 &&
          g >= -1e-6 && g <= 1 + 1e-6 &&
          blue >= -1e-6 && blue <= 1 + 1e-6
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
    mode: ImmersiveColorScheme
  ): { color: string; luminance: number } => {
    // 1. 将不透明像素转入 OKLab 空间。
    //    - album：整张封面均匀取样，选出最具代表性的主题色
    //    - fusion：只保留最右侧 5%（x >= 0.95）的羽化条带，其余像素不参与
    const width = Math.round(Math.sqrt(pixels.length / 4))
    const pts: { L: number; a: number; b: number }[] = []
    for (let p = 0; p * 4 < pixels.length; p++) {
      const i = p * 4
      if (pixels[i + 3] < 128) continue
      const x = width > 1 ? (p % width) / (width - 1) : 1
      if (mode === 'fusion' && x < 0.95) continue
      const { L, a, b } = rgbToOklab(pixels[i], pixels[i + 1], pixels[i + 2])
      pts.push({ L, a, b })
    }

    if (pts.length === 0) {
      // 无有效像素 => 返回深灰色后备（OKLab L≈0.19）
      return { color: 'rgb(40, 40, 40)', luminance: 0.19 }
    }

    // fusion：直接对条带取平均——融合模式下背景色应贴近右缘真实观感，
    // 平均色是渐隐区颜色的最佳估计（K-Means 选主色在双色条带时反而造成突兀过渡）
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

    // 如果像素极少（例如几乎全透明），直接返回平均色（彩度减半避免浑浊）
    if (pts.length < 20) {
      let sumL = 0,
        suma = 0,
        sumb = 0
      for (const p of pts) {
        sumL += p.L
        suma += p.a
        sumb += p.b
      }
      return finalizeColor(
        sumL / pts.length,
        (suma / pts.length) * 0.5,
        (sumb / pts.length) * 0.5
      )
    }

    // 2. K‑means++ 初始化种子（最多 6 个）
    const k = Math.min(6, pts.length)
    const seeds: typeof pts = []
    // 随机选取第一个
    const firstIdx = Math.floor(Math.random() * pts.length)
    seeds.push({ ...pts[firstIdx] })

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
      if (total === 0) break // 所有点已被选为种子（理论上不会）
      let r = Math.random() * total
      for (let j = 0; j < distSq.length; j++) {
        r -= distSq[j]
        if (r <= 0) {
          seeds.push({ ...pts[j] })
          break
        }
      }
    }

    // 3. K‑Means 迭代
    let centers = seeds.map((s) => [s.L, s.a, s.b] as [number, number, number])
    const assign = new Int32Array(pts.length)

    for (let iter = 0; iter < 10; iter++) {
      // 分配
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

      // 更新中心
      const sums = centers.map(() => ({ n: 0, L: 0, a: 0, b: 0 }))
      for (let p = 0; p < pts.length; p++) {
        const s = sums[assign[p]]
        s.n++
        s.L += pts[p].L
        s.a += pts[p].a
        s.b += pts[p].b
      }

      let maxMove = 0
      const newCenters = sums.map((s, i) => {
        if (s.n === 0) return centers[i]
        const nc: [number, number, number] = [s.L / s.n, s.a / s.n, s.b / s.n]
        const move = Math.hypot(nc[0] - centers[i][0], nc[1] - centers[i][1], nc[2] - centers[i][2])
        if (move > maxMove) maxMove = move
        return nc
      })
      centers = newCenters
      if (maxMove < 1e-5) break // 收敛
    }

    // 4. 计算各簇占比
    const counts = new Array<number>(centers.length).fill(0)
    for (let p = 0; p < pts.length; p++) counts[assign[p]]++

    // 5. 综合打分
    let best = { score: -1, L: 0, a: 0, b: 0 }
    for (let c = 0; c < centers.length; c++) {
      const [L, a, b] = centers[c]
      const share = counts[c] / pts.length
      const chroma = Math.hypot(a, b)

      // 基础分：占比重、彩度高
      let score = Math.sqrt(share) * (0.2 + chroma * 2.5)

      // 偏好中等亮度（适合深色背景）
      if (L >= 0.25 && L <= 0.45) score *= 1.3
      else if (L < 0.2 || L > 0.65) score *= 0.6

      // 抑制灰色
      if (chroma < 0.02) score *= 0.3

      if (score > best.score) {
        best = { score, L, a, b }
      }
    }

    // 6. 亮度压缩 + 色域映射后输出
    return finalizeColor(best.L, best.a, best.b)
  }

  // ===================== 监听封面路径 / 取色模式变化 =====================

  watch(
    [coverPath, mode],
    ([path, currentMode]) => {
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
        .catch(() => {
          if (gen === generation) {
            dominantColor.value = ''
            dominantLuminance.value = null
          }
        })
    },
    { immediate: true }
  )

  return { dominantColor, dominantLuminance }
}