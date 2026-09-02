<template>
  <!-- 错误通知浮层 -->
  <TransitionGroup name="error-notification" tag="div" class="error-notifications">
    <div
      v-for="notification in errorNotifications"
      :key="notification.id"
      :class="['error-notification', `error-notification--${notification.severity}`]"
      @click="removeError(notification.id)"
    >
      <span class="material-symbols-rounded error-icon">
        {{
          notification.severity === 'error'
            ? 'error'
            : notification.severity === 'warning'
              ? 'warning'
              : notification.severity === 'success'
                ? 'check_circle'
                : 'info'
        }}
      </span>
      <span class="error-message">{{ notification.message }}</span>
      <button class="error-close" @click.stop="removeError(notification.id)">
        <span class="material-symbols-rounded">close</span>
      </button>
    </div>
  </TransitionGroup>
</template>

<script setup lang="ts">
import { useErrorNotification } from '@/composables/useErrorNotification'

/**
 * 全局错误通知浮层。从 App.vue 拆出:
 * 通知源为模块级单例(useErrorNotification),组件卸载不影响其他消费者。
 */
const { errorNotifications, removeError } = useErrorNotification()
</script>

<style scoped>
.error-notifications {
  position: fixed;
  top: 80px;
  right: 20px;
  z-index: 10000;
  display: flex;
  flex-direction: column;
  gap: 12px;
  pointer-events: none;
}

.error-notification {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 12px 16px;
  border-radius: var(--md-sys-shape-corner-medium);
  box-shadow: 0 2px 8px rgba(0, 0, 0, 0.15);
  min-width: 300px;
  max-width: 500px;
  pointer-events: auto;
  cursor: pointer;
  transition: all 0.2s ease;
}

.error-notification:hover {
  transform: translateX(-4px);
  box-shadow: 0 4px 12px rgba(0, 0, 0, 0.2);
}

.error-notification--error {
  border-left: 4px solid var(--md-sys-color-error);
  background-color: var(--md-sys-color-error-container);
  color: var(--md-sys-color-on-error-container);
}

.error-notification--warning {
  border-left: 4px solid #f59e0b;
  background-color: #fef3c7;
  color: #92400e;
}

.error-notification--info {
  border-left: 4px solid var(--md-sys-color-on-primary-container);
  background-color: var(--md-sys-color-secondary-container);
  color: var(--md-sys-color-on-primary-container);
}

.error-notification--success {
  border-left: 4px solid #22c55e;
  background-color: #dcfce7;
  color: #166534;
}

.error-icon {
  font-size: 24px;
  flex-shrink: 0;
}

.error-message {
  flex: 1;
  font-size: 14px;
  line-height: 1.4;
  word-break: break-word;
}

.error-close {
  background: none;
  border: none;
  padding: 4px;
  cursor: pointer;
  display: flex;
  align-items: center;
  justify-content: center;
  color: inherit;
  opacity: 0.7;
  transition: opacity 0.2s ease;
  flex-shrink: 0;
}

.error-close:hover {
  opacity: 1;
}

.error-close .material-symbols-rounded {
  font-size: 20px;
}

/* 错误通知动画 */
.error-notification-enter-active,
.error-notification-leave-active {
  transition: all 0.3s ease;
}

.error-notification-enter-from {
  opacity: 0;
  transform: translateX(100%);
}

.error-notification-leave-to {
  opacity: 0;
  transform: translateX(100%);
}
</style>
