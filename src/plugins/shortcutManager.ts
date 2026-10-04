/** 快捷键管理器:监听全局 keydown,触发插件注册的快捷键 */

import { pluginManager } from './pluginManager'
import logger from '../utils/logger'

interface ShortcutExtension {
  id: string
  name: string
  key: string
  action: () => void | Promise<void>
  pluginId: string
}

class ShortcutManager {
  private isListening: boolean = false

  constructor() {
    this.handleKeyDown = this.handleKeyDown.bind(this)
  }

  start(): void {
    if (this.isListening) return

    window.addEventListener('keydown', this.handleKeyDown)
    this.isListening = true
    logger.info('快捷键管理器已启动')
  }

  stop(): void {
    if (!this.isListening) return

    window.removeEventListener('keydown', this.handleKeyDown)
    this.isListening = false
    logger.info('快捷键管理器已停止')
  }

  private handleKeyDown(event: KeyboardEvent): void {
    // 焦点在输入框或可编辑区时不抢按键
    const target = event.target as HTMLElement
    if (target.tagName === 'INPUT' || target.tagName === 'TEXTAREA' || target.isContentEditable) {
      return
    }

    const keys: string[] = []
    if (event.ctrlKey) keys.push('ctrl')
    if (event.altKey) keys.push('alt')
    if (event.shiftKey) keys.push('shift')
    if (event.metaKey) keys.push('meta')

    let key = event.key.toLowerCase()
    // 规范化特殊键名,与插件注册串的写法一致
    if (key === ' ') key = 'space'
    if (key === 'escape') key = 'esc'

    // 修饰键已收集过,避免重复加入
    if (!['control', 'alt', 'shift', 'meta'].includes(key)) {
      keys.push(key)
    }

    if (
      keys.length === 0 ||
      (keys.length === 1 && ['ctrl', 'alt', 'shift', 'meta'].includes(keys[0]!))
    ) {
      return
    }

    const pressedKey = keys
      .sort((a, b) => {
        const order: Record<string, number> = { ctrl: 0, alt: 1, shift: 2, meta: 3 }
        return (order[a] ?? 4) - (order[b] ?? 4)
      })
      .join('+')

    // 按键串精确匹配;冲突在注册侧已挡住(见 pluginAPI 的 shortcuts.register),多匹配只作防御性告警
    const shortcuts = pluginManager.getExtensions('shortcuts') as ShortcutExtension[]
    const matched = shortcuts.filter((s) => s.key === pressedKey)
    if (matched.length > 1) {
      logger.warn(
        `快捷键 ${pressedKey} 存在 ${matched.length} 个冲突注册，仅触发首个 (${matched[0]!.name})`,
      )
    }
    const shortcut = matched[0]

    if (shortcut) {
      event.preventDefault()
      event.stopPropagation()

      logger.debug(`触发快捷键: ${shortcut.name} (${pressedKey})`)

      try {
        const result = shortcut.action()
        // 异步失败单独记录,不影响其他快捷键
        if (result instanceof Promise) {
          result.catch((err) => {
            logger.error(`快捷键执行失败: ${shortcut.name}`, err)
          })
        }
      } catch (error) {
        logger.error(`快捷键执行失败: ${shortcut.name}`, error)
      }
    }
  }

  getAllShortcuts(): ShortcutExtension[] {
    return pluginManager.getExtensions('shortcuts') as ShortcutExtension[]
  }
}

export const shortcutManager = new ShortcutManager()
