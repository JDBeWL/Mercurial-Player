/** 错误通知 composable: 模块级通知队列, 并把 errorHandler 的错误桥接成用户可见通知 */

import { ref } from 'vue'
import errorHandler, { AppError, ErrorSeverity } from '../utils/errorHandler'

interface ErrorNotification {
  id: number
  message: string
  severity: 'error' | 'warning' | 'info' | 'success'
  duration: number
  timestamp: Date
}

/** 通知队列 (模块级单例, 所有调用方共享同一份) */
const errorNotifications = ref<ErrorNotification[]>([])
const maxNotifications = 5
// 每条通知挂一个自动关闭的 timeout: 通知以任何方式离开队列 (超出上限被丢弃 / 手动移除 / 清空) 时都必须清掉它
const activeTimeouts = new Map<number, ReturnType<typeof setTimeout>>()

// errorHandler 桥接监听器只注册一次: 设置页等组件反复挂载会让同一条错误渲染成多条通知
let bridgeUnsubscribe: (() => void) | null = null

export function useErrorNotification() {
  const showError = (
    message: string,
    severity: 'error' | 'warning' | 'info' | 'success' = 'error',
    duration: number = 5000,
  ): number => {
    const notification: ErrorNotification = {
      id: Date.now() + Math.random(),
      message,
      severity,
      duration,
      timestamp: new Date(),
    }

    errorNotifications.value.push(notification)

    if (errorNotifications.value.length > maxNotifications) {
      const removed = errorNotifications.value.shift()
      if (removed && activeTimeouts.has(removed.id)) {
        clearTimeout(activeTimeouts.get(removed.id))
        activeTimeouts.delete(removed.id)
      }
    }

    // duration 单位 ms, <= 0 表示常驻不自动关闭
    if (duration > 0) {
      const timeoutId = setTimeout(() => {
        removeError(notification.id)
        activeTimeouts.delete(notification.id)
      }, duration)
      activeTimeouts.set(notification.id, timeoutId)
    }

    return notification.id
  }

  /** 成功通知: 'success' 是独立严重程度 (App.vue 渲染对应样式), 默认 3 秒关闭; 传 title 时合成 "title: message" 单行 */
  const showSuccess = (message: string, title?: string, duration: number = 3000): number =>
    showError(title ? `${title}: ${message}` : message, 'success', duration)

  const removeError = (id: number): void => {
    const index = errorNotifications.value.findIndex((n) => n.id === id)
    if (index > -1) {
      errorNotifications.value.splice(index, 1)
      if (activeTimeouts.has(id)) {
        clearTimeout(activeTimeouts.get(id))
        activeTimeouts.delete(id)
      }
    }
  }

  const clearErrors = (): void => {
    activeTimeouts.forEach((timeoutId) => {
      clearTimeout(timeoutId)
    })
    activeTimeouts.clear()
    errorNotifications.value = []
  }

  // 首次调用才注册桥接, 见 bridgeUnsubscribe 声明处
  if (!bridgeUnsubscribe) {
    bridgeUnsubscribe = errorHandler.onError(
      (error: AppError, options: { showToUser: boolean; userMessage: string }) => {
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
      },
    )
  }

  // unsubscribe 注销全局桥接并复位单例, 之后的 useErrorNotification() 调用会重新注册
  const unsubscribe = (): void => {
    if (bridgeUnsubscribe) {
      bridgeUnsubscribe()
      bridgeUnsubscribe = null
    }
  }

  return {
    errorNotifications,
    showError,
    showSuccess,
    removeError,
    clearErrors,
    unsubscribe,
  }
}
