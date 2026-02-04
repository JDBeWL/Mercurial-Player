import { ErrorType } from './errorHandler'

function toMessage(err: unknown): string {
  if (!err) return ''
  if (typeof err === 'string') return err
  if (err instanceof Error) return err.message || String(err)
  try {
    // tauri invoke errors sometimes come as objects
    const anyErr = err as any
    if (typeof anyErr?.message === 'string') return anyErr.message
    if (typeof anyErr?.error === 'string') return anyErr.error
    if (typeof anyErr?.toString === 'function') return anyErr.toString()
    return JSON.stringify(err)
  } catch {
    return String(err)
  }
}

/**
 * 将后端（Tauri/Rust）返回的错误信息，映射到更精确的前端 ErrorType。
 *
 * 目标：把“格式探测/解码失败”与“音频设备/输出流失败”区分开，避免误提示用户检查音频设备。
 */
export function classifyAudioInvokeError(err: unknown): ErrorType {
  const msg = toMessage(err)
  const m = msg.toLowerCase()

  // --- 解码/格式探测类（你提到的场景） ---
  const decodePatterns = [
    'unrecognized format',
    'failed to probe format',
    'no audio track found',
    'failed to create decoder',
    'decode error',
    'decoder failed',
    'end of stream',
    'unexpected eof',
    'unsupported', // e.g. unsupported audio buffer format / codec
  ]
  if (decodePatterns.some(p => m.includes(p))) {
    return ErrorType.AUDIO_DECODE_ERROR
  }

  // --- 设备/输出流类 ---
  const devicePatterns = [
    'audio device not found',
    'failed to get output devices',
    'failed to open output stream',
    'failed to create output stream',
    'failed to create output stream builder',
    'failed to initialize wasapi',
    'wasapi',
    'device may be in use',
    'no default output device',
    'not initialized',
  ]
  if (devicePatterns.some(p => m.includes(p))) {
    return ErrorType.AUDIO_DEVICE_ERROR
  }

  return ErrorType.AUDIO_PLAYBACK_ERROR
}


