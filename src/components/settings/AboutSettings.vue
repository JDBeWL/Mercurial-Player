<template>
  <div class="tab-content">
    <div class="content-header">
      <h3>{{ $t('config.about') }}</h3>
    </div>
    
    <div class="about-card">
      <div class="app-name">Mercurial Player</div>
      <div class="app-version">v{{ appVersion }}</div>
    </div>
    
    <div class="settings-section">
      <h4 class="section-title">{{ $t('config.links') }}</h4>
      
      <div class="link-item" @click="openGitHub">
        <div class="link-icon">
          <svg viewBox="0 0 24 24" width="24" height="24">
            <path fill="currentColor" d="M12 0c-6.626 0-12 5.373-12 12 0 5.302 3.438 9.8 8.207 11.387.599.111.793-.261.793-.577v-2.234c-3.338.726-4.033-1.416-4.033-1.416-.546-1.387-1.333-1.756-1.333-1.756-1.089-.745.083-.729.083-.729 1.205.084 1.839 1.237 1.839 1.237 1.07 1.834 2.807 1.304 3.492.997.107-.775.418-1.305.762-1.604-2.665-.305-5.467-1.334-5.467-5.931 0-1.311.469-2.381 1.236-3.221-.124-.303-.535-1.524.117-3.176 0 0 1.008-.322 3.301 1.23.957-.266 1.983-.399 3.003-.404 1.02.005 2.047.138 3.006.404 2.291-1.552 3.297-1.23 3.297-1.23.653 1.653.242 2.874.118 3.176.77.84 1.235 1.911 1.235 3.221 0 4.609-2.807 5.624-5.479 5.921.43.372.823 1.102.823 2.222v3.293c0 .319.192.694.801.576 4.765-1.589 8.199-6.086 8.199-11.386 0-6.627-5.373-12-12-12z"/>
          </svg>
        </div>
        <div class="link-info">
          <span class="link-label">GitHub</span>
          <span class="link-url">{{ githubUrl }}</span>
        </div>
        <span class="material-symbols-rounded link-arrow">open_in_new</span>
      </div>
    </div>
    
    <div class="settings-section">
      <h4 class="section-title">{{ $t('config.techStack') }}</h4>
      
      <div class="tech-list">
        <div class="tech-item">
          <span class="tech-name">Tauri</span>
          <span class="tech-desc">{{ $t('config.techTauri') }}</span>
        </div>
        <div class="tech-item">
          <span class="tech-name">Vue 3</span>
          <span class="tech-desc">{{ $t('config.techVue') }}</span>
        </div>
        <div class="tech-item">
          <span class="tech-name">Symphonia</span>
          <span class="tech-desc">{{ $t('config.techSymphonia') }}</span>
        </div>
      </div>
    </div>
  </div>
</template>

<script setup>
import { ref, onMounted } from 'vue'
import { getVersion } from '@tauri-apps/api/app'
import { open } from '@tauri-apps/plugin-shell'
import logger from '../../utils/logger'

const appVersion = ref('0.0.0')
const githubUrl = 'https://github.com/JDBeWL/Mercurial-Player'

const loadAppVersion = async () => {
  try {
    appVersion.value = await getVersion()
  } catch (error) {
    logger.error('Failed to get app version:', error)
  }
}

const openGitHub = async () => {
  try {
    await open(githubUrl)
  } catch (error) {
    logger.error('Failed to open GitHub:', error)
  }
}

onMounted(() => {
  loadAppVersion()
})
</script>

<style scoped>
.tab-content {
  max-width: 720px;
}

.content-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  margin-bottom: 24px;
}

.content-header h3 {
  margin: 0;
  font-size: 24px;
  font-weight: 400;
  color: var(--md-sys-color-on-surface);
}

.about-card {
  display: flex;
  flex-direction: column;
  align-items: center;
  padding: 48px 24px;
  margin-bottom: 32px;
  background-color: var(--md-sys-color-surface-container);
  border-radius: 24px;
}

.app-name {
  font-size: 24px;
  font-weight: 500;
  color: var(--md-sys-color-on-surface);
  margin-bottom: 4px;
}

.app-version {
  font-size: 14px;
  color: var(--md-sys-color-primary);
}

.settings-section {
  margin-bottom: 32px;
}

.section-title {
  font-size: 14px;
  font-weight: 500;
  color: var(--md-sys-color-primary);
  margin: 0 0 16px 16px;
  text-transform: uppercase;
  letter-spacing: 0.5px;
}

.link-item {
  display: flex;
  align-items: center;
  gap: 16px;
  padding: 16px;
  border-radius: 12px;
  cursor: pointer;
  transition: background-color 0.2s ease;
}

.link-item:hover {
  background-color: var(--md-sys-color-surface-container);
}

.link-icon {
  width: 40px;
  height: 40px;
  display: flex;
  align-items: center;
  justify-content: center;
  background-color: var(--md-sys-color-surface-container-high);
  border-radius: 12px;
  color: var(--md-sys-color-on-surface);
}

.link-info {
  flex: 1;
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.link-label {
  font-size: 16px;
  color: var(--md-sys-color-on-surface);
}

.link-url {
  font-size: 12px;
  color: var(--md-sys-color-on-surface-variant);
}

.link-arrow {
  font-size: 20px;
  color: var(--md-sys-color-on-surface-variant);
}

.tech-list {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.tech-item {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding: 12px 16px;
  border-radius: 12px;
  background-color: var(--md-sys-color-surface-container);
}

.tech-name {
  font-size: 14px;
  font-weight: 500;
  color: var(--md-sys-color-on-surface);
}

.tech-desc {
  font-size: 12px;
  color: var(--md-sys-color-on-surface-variant);
}
</style>
