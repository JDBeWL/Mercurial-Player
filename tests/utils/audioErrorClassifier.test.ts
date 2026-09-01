import { describe, it, expect } from 'vitest'
import { classifyAudioInvokeError } from '@/utils/audioErrorClassifier'
import { ErrorType } from '@/utils/errorHandler'

describe('classifyAudioInvokeError', () => {
  it('should classify decode/probe errors as AUDIO_DECODE_ERROR', () => {
    expect(classifyAudioInvokeError(new Error('Failed to probe format: end of stream'))).toBe(
      ErrorType.AUDIO_DECODE_ERROR,
    )
    expect(classifyAudioInvokeError(new Error('Unrecognized format'))).toBe(
      ErrorType.AUDIO_DECODE_ERROR,
    )
    expect(classifyAudioInvokeError('No audio track found')).toBe(ErrorType.AUDIO_DECODE_ERROR)
  })

  it('should classify device/stream errors as AUDIO_DEVICE_ERROR', () => {
    expect(
      classifyAudioInvokeError(new Error('Failed to open output stream: device not available')),
    ).toBe(ErrorType.AUDIO_DEVICE_ERROR)
    expect(classifyAudioInvokeError('Audio device not found: Speakers')).toBe(
      ErrorType.AUDIO_DEVICE_ERROR,
    )
    expect(
      classifyAudioInvokeError('Failed to initialize WASAPI exclusive mode: access denied'),
    ).toBe(ErrorType.AUDIO_DEVICE_ERROR)
  })

  it('should classify Chinese-locale device errors as AUDIO_DEVICE_ERROR', () => {
    expect(classifyAudioInvokeError('无法初始化音频设备: 拒绝访问')).toBe(
      ErrorType.AUDIO_DEVICE_ERROR,
    )
    expect(classifyAudioInvokeError('音频设备不可用')).toBe(ErrorType.AUDIO_DEVICE_ERROR)
    expect(classifyAudioInvokeError('另一个程序正在使用该音频设备')).toBe(
      ErrorType.AUDIO_DEVICE_ERROR,
    )
  })

  it('should NOT report internal state errors as device errors', () => {
    // "not initialized" 是播放器/解码器内部状态错误,不是音频设备故障
    expect(classifyAudioInvokeError('WASAPI player not initialized')).toBe(
      ErrorType.AUDIO_PLAYBACK_ERROR,
    )
    expect(classifyAudioInvokeError('Decoder not initialized')).toBe(ErrorType.AUDIO_PLAYBACK_ERROR)
  })

  it('should default to AUDIO_PLAYBACK_ERROR when unknown', () => {
    expect(classifyAudioInvokeError(new Error('some other error'))).toBe(
      ErrorType.AUDIO_PLAYBACK_ERROR,
    )
  })
})
