/// 封面高清放大的离线 worker：解码、靠左方形裁剪、pica Lanczos3 放大全部在
/// worker 线程完成，主线程不阻塞。通过 Vite 的文件式 worker（非 blob:）加载，
/// 满足 CSP 的 script-src 'self'（blob: worker 被禁止，pica 自身的 'ww' 通道不可用，
/// 因此这里显式禁用 'ww'，让 Lanczos 数学在 worker 线程内联执行）。

import createPica from 'pica'

const pica = createPica({ features: ['js', 'wasm'] })

interface UpscaleRequest {
  id: number
  blob: Blob
  targetSide: number
}

interface UpscaleResponse {
  id: number
  needUpscale?: boolean
  buffer?: ArrayBuffer
  error?: string
}

// 避免 lib.webworker 类型与 DOM 冲突，用最小结构类型收窄 self
const workerSelf = self as unknown as {
  onmessage: ((ev: MessageEvent<UpscaleRequest>) => void) | null
  postMessage: (msg: UpscaleResponse, transfer?: Transferable[]) => void
}

workerSelf.onmessage = async (ev: MessageEvent<UpscaleRequest>) => {
  const { id, blob, targetSide } = ev.data
  try {
    const bitmap = await createImageBitmap(blob)
    const side = Math.min(bitmap.width, bitmap.height)

    // 与主线程回退路径相同的判定：源图足够大时无需放大
    if (side <= 0 || side >= targetSide * 0.98) {
      bitmap.close()
      workerSelf.postMessage({ id, needUpscale: false })
      return
    }

    // 与 CSS（object-fit: cover + object-position: left center）保持一致：
    // 宽图取左上角方形，高图垂直居中
    const src = new OffscreenCanvas(side, side)
    const srcCtx = src.getContext('2d', { willReadFrequently: true })
    if (!srcCtx) {
      bitmap.close()
      workerSelf.postMessage({ id, error: 'no 2d context' })
      return
    }
    srcCtx.drawImage(bitmap, 0, (bitmap.height - side) / 2, side, side, 0, 0, side, side)
    bitmap.close()

    const dst = new OffscreenCanvas(targetSide, targetSide)
    await pica.resize(src as unknown as HTMLCanvasElement, dst as unknown as HTMLCanvasElement, {
      filter: 'lanczos3',
    })

    const out = await dst.convertToBlob({ type: 'image/png' })
    const buffer = await out.arrayBuffer()
    workerSelf.postMessage({ id, buffer }, [buffer])
  } catch (e) {
    workerSelf.postMessage({ id, error: e instanceof Error ? e.message : String(e) })
  }
}

export {}
