/**
 * 快捷键管理器
 * 监听全局键盘事件，触发插件注册的快捷键
 */

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

  /**
   * 启动快捷键监听
   */
  start(): void {
    if (this.isListening) return
    
    window.addEventListener('keydown', this.handleKeyDown)
    this.isListening = true
    logger.info('快捷键管理器已启动')
  }

  /**
   * 停止快捷键监听
   */
  stop(): void {
    if (!this.isListening) return
    
    window.removeEventListener('keydown', this.handleKeyDown)
    this.isListening = false
    logger.info('快捷键管理器已停止')
  }

  /**
   * 处理键盘事件
   */
  private handleKeyDown(event: KeyboardEvent): void {
    // 如果焦点在输入框中，不处理快捷键
    const target = event.target as HTMLElement
    if (target.tagName === 'INPUT' || target.tagName === 'TEXTAREA' || target.isContentEditable) {
      return
    }

    // 构建当前按下的快捷键组合
    const keys: string[] = []
    if (event.ctrlKey) keys.push('ctrl')
    if (event.altKey) keys.push('alt')
    if (event.shiftKey) keys.push('shift')
    if (event.metaKey) keys.push('meta')
    
    // 获取按下的主键
    let key = event.key.toLowerCase()
    // 处理特殊键名
    if (key === ' ') key = 'space'
    if (key === 'escape') key = 'esc'
    
    // 避免重复添加修饰键
    if (!['control', 'alt', 'shift', 'meta'].includes(key)) {
      keys.push(key)
    }
    
    // 如果只有修饰键，不处理
    if (keys.length === 0 || (keys.length === 1 && ['ctrl', 'alt', 'shift', 'meta'].includes(keys[0]))) {
      return
    }

    const pressedKey = keys.sort((a, b) => {
      const order: Record<string, number> = { ctrl: 0, alt: 1, shift: 2, meta: 3 }
      return (order[a] ?? 4) - (order[b] ?? 4)
    }).join('+')

    // 查找匹配的快捷键
    const shortcuts = pluginManager.getExtensions('shortcuts') as ShortcutExtension[]
    const matched = shortcuts.find(s => s.key === pressedKey)

    if (matched) {
      event.preventDefault()
      event.stopPropagation()
      
      logger.debug(`触发快捷键: ${matched.name} (${pressedKey})`)
      
      try {
        const result = matched.action()
        // 如果返回 Promise，等待它完成
        if (result instanceof Promise) {
          result.catch(err => {
            logger.error(`快捷键执行失败: ${matched.name}`, err)
          })
        }
      } catch (error) {
        logger.error(`快捷键执行失败: ${matched.name}`, error)
      }
    }
  }

  /**
   * 获取所有已注册的快捷键
   */
  getAllShortcuts(): ShortcutExtension[] {
    return pluginManager.getExtensions('shortcuts') as ShortcutExtension[]
  }
}

export const shortcutManager = new ShortcutManager()
export default shortcutManager
