<template>
  <!-- Teleport 到 body：抽屉、底部栏都建了自己的层叠上下文，只有挂到 body 上遮罩才能盖住整个视口。
       v-if 放在 Teleport 上：隐藏时不给 body 留占位注释节点，也避开 Teleport + 内部 v-if 的锚点复用 -->
  <Teleport v-if="open" to="body">
    <div class="md3-dialog-overlay" :style="{ zIndex }" @click.self="emit('close')">
      <div
        class="md3-dialog"
        role="dialog"
        aria-modal="true"
        :aria-label="title"
        :style="{
          '--md3-dialog-max-width': maxWidth,
          '--md3-dialog-max-height': maxHeight,
        }"
      >
        <div class="md3-dialog-header">
          <h2 class="md3-dialog-title">{{ title }}</h2>
          <button
            type="button"
            class="icon-button md3-dialog-close"
            :aria-label="$t('common.close')"
            :title="$t('common.close')"
            @click="emit('close')"
          >
            <span class="material-symbols-rounded">close</span>
          </button>
        </div>

        <div v-if="$slots.supporting" class="md3-dialog-supporting">
          <slot name="supporting" />
        </div>

        <!-- 外壳只负责不滚动的那部分：内容区是 flex 列，需要滚动的区域自己声明 overflow -->
        <div class="md3-dialog-body">
          <slot />
        </div>

        <div v-if="$slots.footer" class="md3-dialog-footer">
          <slot name="footer" />
        </div>
      </div>
    </div>
  </Teleport>
</template>

<script setup lang="ts">
withDefaults(
  defineProps<{
    open: boolean
    title: string
    /** 层叠层级：默认与抽屉、通知条错开；需要压住 toast 的弹窗自己传更高的值 */
    zIndex?: number
    /** 宽屏下的尺寸上限；紧凑窗口一律满屏，这两个值不生效 */
    maxWidth?: string
    maxHeight?: string
  }>(),
  { zIndex: 3000, maxWidth: '560px', maxHeight: '80dvh' },
)

const emit = defineEmits<{ close: [] }>()
</script>

<style scoped>
/* M3 对话框基线：headline-small 24/400、24dp 内边距、extra-large 28dp 圆角、level3 投影、
   32% scrim；页脚一律 text button（M3 基础对话框不给确认键填充实色）。
   紧凑窗口（M3 compact < 600dp）换成 full-screen 形态：占满视口、直角、页脚避开导航条。
   这里只用主题真实存在的 token，也不写深浅色兜底值（深色兜底会让浅色模式变成深底深字）。 */
.md3-dialog-overlay {
  position: fixed;
  inset: 0;
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 24px;
  /* 前一条是给 Android System WebView 110 的兜底：color-mix() 要 Chrome 111+，
     不支持时整条声明被丢弃，遮罩会变成全透明 */
  background: rgba(0, 0, 0, 0.32);
  background: color-mix(in srgb, var(--md-sys-color-scrim) 32%, transparent);
  animation: md3-scrim-in 150ms linear;
}

.md3-dialog {
  display: flex;
  flex-direction: column;
  /* 别把 var 写进 min()：CSS 压缩会折成回退值，自定义属性失效；宽度上限交给 max-width。 */
  width: 100%;
  max-width: var(--md3-dialog-max-width, 560px);
  max-height: var(--md3-dialog-max-height, 80dvh);
  /* 供子组件用 @container 判断弹窗自身宽度（视口宽 ≠ 弹窗宽） */
  container-type: inline-size;
  border-radius: var(--md-sys-shape-corner-extra-large, 28px);
  background: var(--md-sys-color-surface);
  color: var(--md-sys-color-on-surface);
  box-shadow: var(--md-sys-elevation-level3);
  overflow: hidden;
  animation: md3-dialog-in 250ms cubic-bezier(0.05, 0.7, 0.1, 1);
}

@keyframes md3-scrim-in {
  from {
    opacity: 0;
  }
  to {
    opacity: 1;
  }
}

@keyframes md3-dialog-in {
  from {
    opacity: 0;
    transform: scale(0.92);
  }
  to {
    opacity: 1;
    transform: none;
  }
}

@keyframes md3-sheet-in {
  from {
    opacity: 0;
    transform: translateY(24px);
  }
  to {
    opacity: 1;
    transform: none;
  }
}

@media (prefers-reduced-motion: reduce) {
  .md3-dialog-overlay,
  .md3-dialog {
    animation: none;
  }
}

.md3-dialog-header {
  display: flex;
  flex: none;
  align-items: center;
  gap: 16px;
  padding: 24px 24px 0;
}

.md3-dialog-title {
  flex: 1;
  min-width: 0;
  margin: 0;
  font-size: 24px;
  font-weight: 400;
  line-height: 32px;
  overflow-wrap: anywhere;
}

/* 关闭按钮复用全站 .icon-button：40×40、16px 圆角的矩形，与标题栏、播放器的图标按钮一致。
   这里只保留 flex: none —— 标题很长时按钮不能被挤压变形。
   之前单独写成 48×48 的圆形，是照 MD3 规范来的，但本应用整体走的是圆角矩形，
   歌词候选框和 BPM 分析弹窗因此看起来像"另一个组件库的东西"。 */
.md3-dialog-close {
  flex: none;
}

.md3-dialog-supporting {
  flex: none;
  padding: 8px 24px 0;
  color: var(--md-sys-color-on-surface-variant);
  font-size: 14px;
  line-height: 20px;
}

.md3-dialog-body {
  display: flex;
  flex: 1 1 auto;
  min-height: 0;
  flex-direction: column;
  padding: 16px 24px 0;
  overflow: hidden;
}

.md3-dialog-footer {
  display: flex;
  flex: none;
  justify-content: flex-end;
  gap: 8px;
  padding: 16px 24px 24px;
}

/* M3 对话框的按钮是 text button：填满槽位里的按钮由外壳统一给度量，
   消费方只写文案与行为 */
.md3-dialog-footer :deep(button) {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  gap: 8px;
  min-width: 64px;
  height: 40px;
  padding: 0 12px;
  border: none;
  border-radius: 999px;
  background: transparent;
  color: var(--md-sys-color-primary);
  font-family: inherit;
  font-size: 14px;
  font-weight: 500;
  letter-spacing: 0.1px;
  cursor: pointer;
  transition: background-color 0.2s ease;
}

.md3-dialog-footer :deep(button:not(:disabled):hover) {
  background-color: var(--md-sys-color-hover-overlay);
}

.md3-dialog-footer :deep(button:disabled) {
  /* 用 opacity 而不是 color-mix()，理由见 overlay 的兜底注释 */
  opacity: 0.38;
  cursor: not-allowed;
}

.md3-dialog-footer :deep(button .material-symbols-rounded) {
  font-size: 18px;
}

@media (max-width: 600px) {
  .md3-dialog-overlay {
    align-items: stretch;
    padding: 0;
  }

  .md3-dialog {
    max-width: none;
    max-height: 100%;
    border-radius: 0;
    box-shadow: none;
    animation: md3-sheet-in 250ms cubic-bezier(0.05, 0.7, 0.1, 1);
  }

  .md3-dialog-header {
    padding: calc(12px + env(safe-area-inset-top, 0px)) 8px 0 16px;
  }

  .md3-dialog-title {
    font-size: 20px;
    line-height: 28px;
  }

  .md3-dialog-supporting {
    padding: 4px 16px 0;
  }

  .md3-dialog-body {
    padding: 12px 16px 0;
  }

  .md3-dialog-footer {
    padding: 12px 16px calc(12px + env(safe-area-inset-bottom));
  }
}
</style>
