/**
 * 错误通知 Composable
 * 
 * 提供统一的错误通知机制，用于向用户显示友好的错误消息
 */

import { ref } from 'vue';
import errorHandler from '../utils/errorHandler';

/**
 * 错误通知状态
 */
const errorNotifications = ref([]);
const maxNotifications = 5;
// 存储所有活动的 timeout ID，用于清理
const activeTimeouts = new Map();

/**
 * 使用错误通知
 */
export function useErrorNotification() {
  /**
   * 显示错误通知
   * @param {string} message - 错误消息
   * @param {string} severity - 错误严重程度
   * @param {number} duration - 显示时长（毫秒），0 表示不自动关闭
   */
  const showError = (message, severity = 'error', duration = 5000) => {
    const notification = {
      id: Date.now() + Math.random(),
      message,
      severity, // 'error', 'warning', 'info'
      duration,
      timestamp: new Date()
    };

    errorNotifications.value.push(notification);

    // 限制通知数量
    if (errorNotifications.value.length > maxNotifications) {
      const removed = errorNotifications.value.shift();
      // 清理被移除通知的 timeout
      if (removed && activeTimeouts.has(removed.id)) {
        clearTimeout(activeTimeouts.get(removed.id));
        activeTimeouts.delete(removed.id);
      }
    }

    // 自动关闭
    if (duration > 0) {
      const timeoutId = setTimeout(() => {
        removeError(notification.id);
        activeTimeouts.delete(notification.id);
      }, duration);
      activeTimeouts.set(notification.id, timeoutId);
    }

    return notification.id;
  };

  /**
   * 移除错误通知
   * @param {number} id - 通知 ID
   */
  const removeError = (id) => {
    const index = errorNotifications.value.findIndex(n => n.id === id);
    if (index > -1) {
      errorNotifications.value.splice(index, 1);
      // 清理对应的 timeout
      if (activeTimeouts.has(id)) {
        clearTimeout(activeTimeouts.get(id));
        activeTimeouts.delete(id);
      }
    }
  };

  /**
   * 清空所有错误通知
   */
  const clearErrors = () => {
    // 清理所有 timeout
    activeTimeouts.forEach((timeoutId) => {
      clearTimeout(timeoutId);
    });
    activeTimeouts.clear();
    errorNotifications.value = [];
  };

  // 注册错误处理器监听器
  const unsubscribe = errorHandler.onError((error, options) => {
    if (options.showToUser) {
      const message = options.userMessage || errorHandler.getUserFriendlyMessage(error);
      const severity = error.severity === 'CRITICAL' || error.severity === 'HIGH' 
        ? 'error' 
        : error.severity === 'MEDIUM' 
        ? 'warning' 
        : 'info';
      
      showError(message, severity);
    }
  });

  return {
    errorNotifications,
    showError,
    removeError,
    clearErrors,
    unsubscribe
  };
}

