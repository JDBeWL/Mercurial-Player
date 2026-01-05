/**
 * 错误通知 Composable
 * 
 * 提供统一的错误通知机制，用于向用户显示友好的错误消息
 */

import { ref } from 'vue'
import errorHandler, { AppError, ErrorSeverity } from '../utils/errorHandler'

interface ErrorNotification {
  id: number
  message: string
  severity: 'error' | 'warning' | 'info'
  duration: number
  timestamp: Date
}

/**
 * 错误通知状态
 */
const errorNotifications = ref<ErrorNotification[]>([])
const maxNotifications = 5
// 存储所有活动的 timeout ID，用于清理
const activeTimeouts = new Map<number, ReturnType<typeof setTimeout>>()

/**
 * 使用错误通知
 */
export function useErrorNotification() {
  /**
   * 显示错误通知
   */
  const showError = (
    message: string,
    severity: 'error' | 'warning' | 'info' = 'error',
    duration: number = 5000
  ): number => {
    const notification: ErrorNotification = {
      id: Date.now() + Math.random(),
      message,
      severity,
      duration,
      timestamp: new Date()
    }

    errorNotifications.value.push(notification)

    // 限制通知数量
    if (errorNotifications.value.length > maxNotifications) {
      const removed = errorNotifications.value.shift()
      // 清理被移除通知的 timeout
      if (removed && activeTimeouts.has(removed.id)) {
        clearTimeout(activeTimeouts.get(removed.id))
        activeTimeouts.delete(removed.id)
      }
    }

    // 自动关闭
    if (duration > 0) {
      const timeoutId = setTimeout(() => {
        removeError(notification.id)
        activeTimeouts.delete(notification.id)
      }, duration)
      activeTimeouts.set(notification.id, timeoutId)
    }

    return notification.id
  }

  /**
   * 移除错误通知
   */
  const removeError = (id: number): void => {
    const index = errorNotifications.value.findIndex(n => n.id === id)
    if (index > -1) {
      errorNotifications.value.splice(index, 1)
      // 清理对应的 timeout
      if (activeTimeouts.has(id)) {
        clearTimeout(activeTimeouts.get(id))
        activeTimeouts.delete(id)
      }
    }
  }

  /**
   * 清空所有错误通知
   */
  const clearErrors = (): void => {
    // 清理所有 timeout
    activeTimeouts.forEach((timeoutId) => {
      clearTimeout(timeoutId)
    })
    activeTimeouts.clear()
    errorNotifications.value = []
  }

  // 注册错误处理器监听器
  const unsubscribe = errorHandler.onError((error: AppError, options: { showToUser: boolean; userMessage: string }) => {
    if (options.showToUser) {
      const message = options.userMessage || errorHandler.getUserFriendlyMessage(error)
      const severity: 'error' | 'warning' | 'info' = 
        error.severity === ErrorSeverity.CRITICAL || error.severity === ErrorSeverity.HIGH 
          ? 'error' 
          : error.severity === ErrorSeverity.MEDIUM 
          ? 'warning' 
          : 'info'
      
      showError(message, severity)
    }
  })

  return {
    errorNotifications,
    showError,
    removeError,
    clearErrors,
    unsubscribe
  }
}
