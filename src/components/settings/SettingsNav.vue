<template>
  <nav class="settings-nav" :data-mobile="isAndroid ? 'true' : undefined">
    <div class="nav-header">
      <h2>{{ $t('config.title') }}</h2>
      <button class="icon-button" :title="$t('common.close')" @click="$emit('close')">
        <span class="material-symbols-rounded">close</span>
      </button>
    </div>

    <div class="nav-items">
      <button
        v-for="tab in tabs"
        :key="tab.id"
        class="nav-item"
        :class="{ active: modelValue === tab.id }"
        @click="onSelect(tab.id)"
      >
        <span class="material-symbols-rounded">{{ tab.icon }}</span>
        <span class="nav-label">{{ $t(tab.label) }}</span>
      </button>
    </div>
  </nav>
</template>

<script setup lang="ts">
import type { SettingsTab } from '@/types'
import { usePlatform } from '@/composables/usePlatform'

defineProps<{
  modelValue: string
  tabs: SettingsTab[]
}>()

const emit = defineEmits<{
  'update:modelValue': [value: string]
  close: []
  /** 手机竖屏靠这个事件切到详情页 —— 只监听 v-model 的话，
      点"当前已选中"的那一项值时不变，watch 不触发就进不去详情 */
  select: [value: string]
}>()

const { isAndroid } = usePlatform()

const onSelect = (id: string): void => {
  emit('update:modelValue', id)
  emit('select', id)
}
</script>

<style scoped>
.settings-nav {
  width: 280px;
  min-width: 240px;
  /* 不画底色：surface 恒等于 background，画上去只会让左栏成为独立的不透明层，换色时与周围对不齐 */
  background-color: transparent;
  display: flex;
  flex-direction: column;
  border-right: 1px solid var(--md-sys-color-outline-variant);
}

.nav-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding: 16px 16px 24px;
}

.nav-header h2 {
  margin: 0;
  font-size: 22px;
  font-weight: 500;
  color: var(--md-sys-color-on-surface);
}

.nav-items {
  flex: 1;
  padding: 0 12px;
  overflow-y: auto;
  scrollbar-width: none;
}

.nav-items::-webkit-scrollbar {
  display: none;
}

.nav-item {
  display: flex;
  align-items: center;
  gap: 12px;
  width: 100%;
  padding: 16px;
  margin-bottom: 4px;
  border: none;
  border-radius: 28px;
  background: none;
  cursor: pointer;
  color: var(--md-sys-color-on-surface-variant);
  font-size: 14px;
  font-weight: 500;
  text-align: left;
  transition: all 0.2s ease;
}

/* 触屏上没有真正的悬停：不加 hover: hover 守卫的话，点过的行会一直停在
   :hover 态，看起来像"选中留下的痕迹"（原为 surface-container-highest：
   主题不输出该角色，声明静默失效、等于无反馈） */
@media (hover: hover) {
  .nav-item:hover {
    background-color: var(--md-sys-color-hover-overlay);
  }

  .icon-button:hover {
    background-color: color-mix(in srgb, var(--md-sys-color-on-surface) 8%, transparent);
  }
}

.nav-item.active {
  background-color: var(--md-sys-color-secondary-container);
  color: var(--md-sys-color-on-secondary-container);
}

.nav-item .material-symbols-rounded {
  font-size: 24px;
}

.nav-label {
  flex: 1;
}

.icon-button {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 40px;
  height: 40px;
  border: none;
  border-radius: var(--md-sys-shape-corner-large);
  background: none;
  cursor: pointer;
  color: var(--md-sys-color-on-surface-variant);
  transition: all 0.2s ease;
}

@media (max-width: 768px) {
  .settings-nav {
    width: 100%;
    min-width: unset;
    border-right: none;
    border-bottom: 1px solid var(--md-sys-color-outline-variant);
  }

  .nav-items {
    display: flex;
    flex-wrap: nowrap;
    overflow-x: auto;
    padding: 8px 12px;
    gap: 8px;
  }

  .nav-item {
    /* 必须重置 width —— 基础样式是给"纵向列表"写的 width:100%，
       横向 flex 下配合 flex-shrink:0 会让每个 tab 撑满整行宽度，
       N 个 tab 就排成 N 屏宽、用户只能看到第一个（实测 10 个 tab
       宽度全是 398px，top 全是 145）。 */
    width: auto;
    flex-shrink: 0;
    padding: 12px 16px;
    margin-bottom: 0;
  }

  .nav-label {
    display: none;
  }
}

/* ===== 手机竖屏：导航改成整屏的入口列表 =====
   上面那条横向 tab 是给"桌面窄窗口"用的：手机上 10 个 tab 横向滚动
   既看不出还有多少页、也点不准。这里换成手机上最标准的「列表 -> 详情」两级，
   竖屏下这一栏就是整屏的入口列表（由 Settings.vue 控制与详情的互斥）。

   形态照 Google Play 的设置页：平铺单行、图标 + 文字、不给选中项铺胶囊底色。
   栅格：容器左右 8px + 行内 8px -> 图标落在 16px，图标后 16px -> 文字落在 56px，
   与 Settings 头部行的标题同一条竖线。

   用 [data-mobile='true'] 守卫：@media (orientation: portrait) 在桌面
   把窗口拉成窄高时同样会命中，不加守卫会把桌面端的导航形态一起改掉。 */
@media (orientation: portrait) {
  .settings-nav[data-mobile='true'] {
    width: 100%;
    min-width: 0;
    flex: 1;
    min-height: 0;
    border-right: none;
    border-bottom: none;
  }

  /* 标题与关闭按钮由 Settings 的头部行统一提供（顶栏此时也已隐藏），这一行整条撤掉 */
  .settings-nav[data-mobile='true'] .nav-header {
    display: none;
  }

  .settings-nav[data-mobile='true'] .nav-items {
    display: block;
    overflow-x: hidden;
    overflow-y: auto;
    padding: 4px 8px calc(8px + env(safe-area-inset-bottom, 0px));
    gap: 0;
  }

  /* 52px 行高：够手指点，又不至于翻两屏才看完一页入口 */
  .settings-nav[data-mobile='true'] .nav-item {
    width: 100%;
    min-height: 52px;
    gap: 16px;
    padding: 8px;
    margin-bottom: 0;
    border-radius: 12px;
    color: var(--md-sys-color-on-surface);
    font-size: 16px;
    font-weight: 400;
  }

  .settings-nav[data-mobile='true'] .nav-item .material-symbols-rounded {
    color: var(--md-sys-color-on-surface-variant);
  }

  /* 两级导航里"哪一项是当前项"没有意义（点一下就进详情），常驻的 secondary-container
     胶囊只会像一处没擦干净的选中痕迹 —— 只保留按压那一瞬间的反馈 */
  .settings-nav[data-mobile='true'] .nav-item.active {
    background-color: transparent;
  }

  .settings-nav[data-mobile='true'] .nav-item:active {
    background-color: var(--md-sys-color-hover-overlay);
  }

  /* 覆盖上面 768px 那条的 display:none —— 手机上纯图标完全看不出是哪一页 */
  .settings-nav[data-mobile='true'] .nav-label {
    display: block;
  }
}
</style>
