<template>
  <nav class="settings-nav">
    <div class="nav-header">
      <h2>{{ $t('config.title') }}</h2>
      <button class="icon-button" @click="$emit('close')" :title="$t('common.close')">
        <span class="material-symbols-rounded">close</span>
      </button>
    </div>
    
    <div class="nav-items">
      <button 
        v-for="tab in tabs" 
        :key="tab.id"
        class="nav-item"
        :class="{ active: modelValue === tab.id }"
        @click="$emit('update:modelValue', tab.id)"
      >
        <span class="material-symbols-rounded">{{ tab.icon }}</span>
        <span class="nav-label">{{ $t(tab.label) }}</span>
      </button>
    </div>
  </nav>
</template>

<script setup>
defineProps({
  modelValue: {
    type: String,
    required: true
  },
  tabs: {
    type: Array,
    required: true
  }
})

defineEmits(['update:modelValue', 'close'])
</script>

<style scoped>
.settings-nav {
  width: 280px;
  min-width: 240px;
  background-color: var(--md-sys-color-surface);
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

.nav-item:hover {
  background-color: var(--md-sys-color-surface-container-highest);
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

.icon-button:hover {
  background-color: color-mix(in srgb, var(--md-sys-color-on-surface) 8%, transparent);
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
    flex-shrink: 0;
    padding: 12px 16px;
    margin-bottom: 0;
  }
  
  .nav-label {
    display: none;
  }
}
</style>
