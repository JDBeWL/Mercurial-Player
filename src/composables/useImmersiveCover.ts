import { ref, watch, onUnmounted, type Ref } from 'vue'
import { readFile } from '@tauri-apps/plugin-fs'
import { convertFileSrc } from '@tauri-apps/api/core'
import createPica from 'pica'
import CoverUpscaleWorker from '../workers/coverUpscale.worker?worker'

/**
 * 沉浸式封面高清放大。
 *
 * 浏览器对 <img> 的默认放大是双线性插值，封面被拉伸到整窗高度后会发糊。
 * 这里按显示端相同的裁剪规则（object-fit: cover + object-position: left center）
 * 先裁出靠左的正方形源区域，再用 pica 的 Lanczos3 核放大到「窗口高度 × DPR」
 * 的精确像素尺寸，最后以 objectURL 交给 <img>，实现 1:1 像素映射。
 *
 * - 优先在专用 worker 线程执行（解码 + 裁剪 + Lanczos 放大），主线程零阻塞，
 *   避免切歌瞬间与歌词首帧渲染争抢主线程。
 * - worker 不可用（创建失败 / 运行报错 / OffscreenCanvas 不支持）时回退
 *   主线程路径（Lanczos 在主线程同步执行，功能不中断但可能短暂卡顿）。
 * - 源图本身足够大（无需放大）时直接使用原始 URL，浏览器缩小显示质量足够。
 * - 任何一步失败都回退原始 URL，功能不中断。
 * - 处理期间先展示原始 URL，完成后无缝替换，避免封面闪空。
 */

// 主线程回退路径用的 pica 单例。features 显式不含 'ww'（web worker）：
// CSP 的 script-src 不允许 blob: worker，pica 的 worker 通道加载失败时不 reject
// 而是永久挂起，必须从源头禁用。wasm 在 'wasm-unsafe-eval' 下编译可用，
// pica 检测失败后自动回退纯 JS 数学内核（仍为 Lanczos 高质量路径）。
const pica = createPica({ features: ['js', 'wasm'] })

// 防御性上限：超高 DPI / 超大窗口时避免生成离谱尺寸的画布
const MAX_OUTPUT_SIDE = 4096

const mimeFromPath = (path: string): string => {
  const ext = path.split('.').pop()?.toLowerCase() ?? ''
  if (ext === 'png') return 'image/png'
  if (ext === 'webp') return 'image/webp'
  return 'image/jpeg'
}

interface DecodedImage {
  source: CanvasImageSource
  width: number
  height: number
  cleanup: () => void
}

const readCoverBlob = async (path: string): Promise<Blob> => {
  if (
    path.startsWith('http://') ||
    path.startsWith('https://') ||
    path.startsWith('data:') ||
    path.startsWith('asset://')
  ) {
    const res = await fetch(path)
    return res.blob()
  }
  const data = await readFile(path)
  return new Blob([data], { type: mimeFromPath(path) })
}

const decodeImage = async (path: string): Promise<DecodedImage> => {
  const blob = await readCoverBlob(path)

  if (typeof createImageBitmap === 'function') {
    const bitmap = await createImageBitmap(blob)
    return {
      source: bitmap,
      width: bitmap.width,
      height: bitmap.height,
      cleanup: () => bitmap.close(),
    }
  }

  const url = URL.createObjectURL(blob)
  const img = await new Promise<HTMLImageElement>((resolve, reject) => {
    const image = new Image()
    image.onload = () => resolve(image)
    image.onerror = () => reject(new Error('image decode failed'))
    image.src = url
  })
  return {
    source: img,
    width: img.naturalWidth,
    height: img.naturalHeight,
    cleanup: () => URL.revokeObjectURL(url),
  }
}

/** 目标边长：沉浸层高度（= 窗口高度）× DPR，向上取整并封顶 */
const displaySide = (): number =>
  Math.min(MAX_OUTPUT_SIDE, Math.round(window.innerHeight * (window.devicePixelRatio || 1)))

/**
 * 裁剪 + Lanczos3 放大（主线程回退路径）。
 * 返回 objectURL；源图足够大无需放大时返回 null（调用方沿用原始 URL）。
 */
const upscaleCoverOnMainThread = async (
  path: string,
  targetSide: number,
): Promise<string | null> => {
  const { source, width, height, cleanup } = await decodeImage(path)
  try {
    // 与 CSS（object-fit: cover + object-position: left center）保持一致：
    // 取靠左的正方形区域（高图垂直居中裁切，宽图右侧裁掉）
    const side = Math.min(width, height)
    if (side <= 0 || side >= targetSide * 0.98) return null

    const srcCanvas = document.createElement('canvas')
    srcCanvas.width = side
    srcCanvas.height = side
    const srcCtx = srcCanvas.getContext('2d', { willReadFrequently: true })
    if (!srcCtx) return null
    // 宽图: sy = (height - side) / 2 = 0，即取左上角方形；高图: 垂直居中
    srcCtx.drawImage(source, 0, (height - side) / 2, side, side, 0, 0, side, side)

    const dstCanvas = document.createElement('canvas')
    dstCanvas.width = targetSide
    dstCanvas.height = targetSide
    await pica.resize(srcCanvas, dstCanvas, { filter: 'lanczos3' })

    const blob = await pica.toBlob(dstCanvas, 'image/png')
    return URL.createObjectURL(blob)
  } finally {
    cleanup()
  }
}

// --- worker 通道：解码 + 裁剪 + Lanczos 全部在 worker 线程执行，主线程零阻塞 ---
// 文件式 worker 从 'self' 加载，不违反 CSP（blob: worker 才被禁止）。
// worker 内任何结构性失败（创建失败 / 加载失败 / OffscreenCanvas 或 pica 报错）
// 都会永久停用该通道，后续请求走主线程回退路径。

interface UpscaleWorkerResponse {
  id: number
  needUpscale?: boolean
  buffer?: ArrayBuffer
  error?: string
}

interface WorkerJob {
  resolve: (url: string | null) => void
  reject: (err: Error) => void
  timer: ReturnType<typeof setTimeout>
}

let upscaleWorker: Worker | null = null
let workerBroken = false
let nextJobId = 0
const pendingJobs = new Map<number, WorkerJob>()

const failPendingJobs = (message: string): void => {
  for (const job of pendingJobs.values()) {
    clearTimeout(job.timer)
    job.reject(new Error(message))
  }
  pendingJobs.clear()
}

const ensureWorker = (): Worker | null => {
  if (workerBroken) return null
  if (upscaleWorker) return upscaleWorker
  try {
    const w = new CoverUpscaleWorker()
    w.onmessage = (ev: MessageEvent<UpscaleWorkerResponse>) => {
      const job = pendingJobs.get(ev.data.id)
      if (!job) return
      pendingJobs.delete(ev.data.id)
      clearTimeout(job.timer)
      if (ev.data.error) {
        // worker 内结构性失败：停用通道，本次请求走回退路径
        workerBroken = true
        try {
          w.terminate()
        } catch {
          /* ignore */
        }
        upscaleWorker = null
        job.reject(new Error(ev.data.error))
      } else if (ev.data.needUpscale) {
        job.resolve(null)
      } else if (ev.data.buffer) {
        job.resolve(URL.createObjectURL(new Blob([ev.data.buffer], { type: 'image/png' })))
      } else {
        job.reject(new Error('worker response missing data'))
      }
    }
    w.onerror = () => {
      // worker 加载失败（脚本/模块错误）：停用通道并触发回退
      workerBroken = true
      upscaleWorker = null
      failPendingJobs('upscale worker failed to load')
      try {
        w.terminate()
      } catch {
        /* ignore */
      }
    }
    upscaleWorker = w
    return w
  } catch {
    workerBroken = true
    return null
  }
}

// worker 通道超时：正常放大在数百毫秒内完成，超时视为通道异常，走回退路径
const WORKER_TIMEOUT_MS = 10_000

const upscaleViaWorker = (path: string, targetSide: number): Promise<string | null> => {
  const worker = ensureWorker()
  if (!worker) return Promise.reject(new Error('upscale worker unavailable'))

  const id = ++nextJobId
  return readCoverBlob(path).then(
    (blob) =>
      new Promise<string | null>((resolve, reject) => {
        const timer = setTimeout(() => {
          pendingJobs.delete(id)
          reject(new Error('upscale worker timeout'))
        }, WORKER_TIMEOUT_MS)
        pendingJobs.set(id, { resolve, reject, timer })
        worker.postMessage({ id, blob, targetSide })
      }),
  )
}

const upscaleCover = async (path: string, targetSide: number): Promise<string | null> => {
  try {
    return await upscaleViaWorker(path, targetSide)
  } catch {
    return upscaleCoverOnMainThread(path, targetSide)
  }
}

export function useImmersiveCover(coverPath: Ref<string | undefined | null>, enabled: Ref<boolean>) {
  const coverDisplayUrl = ref('')
  let generation = 0
  let objectUrl = ''
  let lastProcessedSide = 0
  let resizeTimer: number | undefined

  const revokeProcessed = () => {
    if (objectUrl) {
      URL.revokeObjectURL(objectUrl)
      objectUrl = ''
    }
  }

  const run = () => {
    const gen = ++generation
    const path = coverPath.value

    if (!path) {
      revokeProcessed()
      coverDisplayUrl.value = ''
      lastProcessedSide = 0
      return
    }

    // 先立即展示原始 URL，处理完成后再替换为高清版本
    coverDisplayUrl.value = convertFileSrc(path)

    if (!enabled.value) {
      revokeProcessed()
      lastProcessedSide = 0
      return
    }

    const targetSide = displaySide()
    lastProcessedSide = targetSide
    upscaleCover(path, targetSide)
      .then((url) => {
        // 已切换曲目 / 关闭沉浸模式 / 尺寸已变：丢弃过期结果
        if (gen !== generation || !url) return
        revokeProcessed()
        objectUrl = url
        coverDisplayUrl.value = url
      })
      .catch(() => {
        // 处理失败：保留原始 URL
      })
  }

  watch([coverPath, enabled], run, { immediate: true })

  // 窗口尺寸 / DPI 变化后重新放大（防抖，且仅在目标边长明显变化时重算）
  const onResize = () => {
    if (resizeTimer) clearTimeout(resizeTimer)
    resizeTimer = window.setTimeout(() => {
      if (!enabled.value || !coverPath.value) return
      if (Math.abs(displaySide() - lastProcessedSide) > 32) run()
    }, 300)
  }
  window.addEventListener('resize', onResize)

  onUnmounted(() => {
    generation++
    revokeProcessed()
    if (resizeTimer) clearTimeout(resizeTimer)
    window.removeEventListener('resize', onResize)
  })

  return { coverDisplayUrl }
}
