<template>
  <div class="tab-content">
    <div class="content-header">
      <h3>{{ $t('config.about') }}</h3>
    </div>
    
    <div class="about-card">
      <div class="app-header">
        <div class="app-name">Mercurial Player</div>
        <div class="app-version">v{{ appVersion }}</div>
        <div style="margin-left: auto; display:flex; flex-direction:column; align-items:flex-end; gap:8px;">
          <div>
            <button class="filled-button" @click="checkForUpdates" :disabled="isChecking">
              <span class="material-symbols-rounded" v-if="!isChecking">download</span>
              <span class="material-symbols-rounded spin" v-else>hourglass_empty</span>
              {{ isChecking ? t('config.checkingUpdates') : t('config.checkUpdates') }}
            </button>
          </div>
          <div v-if="lastCheckTime" class="text-caption" style="color:var(--md-sys-color-on-surface-variant);">
            {{ t('config.lastChecked') }}: {{ lastCheckTime }}
          </div>

          <div v-if="error" class="text-caption" style="color:var(--md-sys-color-error); margin-top:6px;">
            {{ error }}
          </div>
          <div v-if="updateLog && !isDownloadFinishedLog(updateLog)" class="text-caption" style="margin-top:6px; color:var(--md-sys-color-on-surface-variant); word-break:break-all;">
            {{ updateLog }}
          </div>

        </div>
      </div>
    </div>
    
    <div class="settings-section">
      <h4 class="section-title">{{ $t('config.links') }}</h4>
      
      <div class="link-item">
        <div class="link-icon">
          <svg viewBox="0 0 24 24" width="24" height="24">
            <path fill="currentColor" d="M12 0c-6.626 0-12 5.373-12 12 0 5.302 3.438 9.8 8.207 11.387.599.111.793-.261.793-.577v-2.234c-3.338.726-4.033-1.416-4.033-1.416-.546-1.387-1.333-1.756-1.333-1.756-1.089-.745.083-.729.083-.729 1.205.084 1.839 1.237 1.839 1.237 1.07 1.834 2.807 1.304 3.492.997.107-.775.418-1.305.762-1.604-2.665-.305-5.467-1.334-5.467-5.931 0-1.311.469-2.381 1.236-3.221-.124-.303-.535-1.524.117-3.176 0 0 1.008-.322 3.301 1.23.957-.266 1.983-.399 3.003-.404 1.02.005 2.047.138 3.006.404 2.291-1.552 3.297-1.23 3.297-1.23.653 1.653.242 2.874.118 3.176.77.84 1.235 1.911 1.235 3.221 0 4.609-2.807 5.624-5.479 5.921.43.372.823 1.102.823 2.222v3.293c0 .319.192.694.801.576 4.765-1.589 8.199-6.086 8.199-11.386 0-6.627-5.373-12-12-12z"/>
          </svg>
        </div>
        <div class="link-info">
          <span class="link-label">GitHub</span>
          <span class="link-url">{{ githubUrl }}</span>
        </div>
        <span class="material-symbols-rounded link-arrow" @click="openGitHub">open_in_new</span>
      </div>
    </div>
    
    <div class="settings-section">
      <h4 class="section-title">{{ $t('config.techStack') }}</h4>
      
      <div class="tech-categories">
        <div 
          v-for="category in techCategories" 
          :key="category.name"
          class="tech-category-card"
        >
          <div class="category-header">
            <span class="category-icon material-symbols-rounded">{{ category.icon }}</span>
            <h5 class="category-title">{{ category.name }}</h5>
          </div>
          <p v-if="category.note" class="category-note">{{ category.note }}</p>
          <div class="tech-list">
            <div 
              v-for="tech in category.techs" 
              :key="tech.name" 
              class="tech-item"
            >
              <div class="tech-info">
                <span class="tech-name">{{ tech.name }}</span>
                <span class="tech-desc">{{ tech.desc }}</span>
              </div>
              <div class="tech-links">
                <button 
                  v-if="tech.docs" 
                  class="icon-button tech-link-btn" 
                  @click.stop="openLink(tech.docs)"
                  :title="$t('config.openDocs')"
                >
                  <span class="material-symbols-rounded">description</span>
                </button>
                <button 
                  v-if="tech.repo" 
                  class="icon-button tech-link-btn" 
                  @click.stop="openLink(tech.repo)"
                  :title="$t('config.openRepo')"
                >
                  <span class="material-symbols-rounded">code</span>
                </button>
              </div>
            </div>
          </div>
        </div>
      </div>
      <div class="settings-section">
        <h4 class="section-title">{{ $t('config.license') }}</h4>
        
        <div class="license-card">
          <button class="license-external-link" @click="openLicense" :title="$t('config.licenseDetails')">
            <span class="material-symbols-rounded">open_in_new</span>
          </button>
          
          <div class="license-content">
            <div class="license-icon">
              <span class="material-symbols-rounded">code</span>
            </div>
            
            <div class="license-main">
              <h3 class="license-title">{{ $t('config.licenseTitle') }}</h3>
              <p class="license-subtitle">{{ $t('config.licenseSubtitle') }}</p>
              <p class="license-copyright">{{ $t('config.copyright') }}</p>
              
              <div v-if="showLicenseDetails" class="license-legal-text">
                <p class="license-paragraph">{{ $t('config.licenseInfo') }}</p>
                <p class="license-paragraph">{{ $t('config.licenseWarranty') }}</p>
              </div>
              
              <button 
                class="license-details-toggle" 
                @click="showLicenseDetails = !showLicenseDetails"
              >
                <span class="material-symbols-rounded">{{ showLicenseDetails ? 'expand_less' : 'expand_more' }}</span>
                <span>{{ showLicenseDetails ? $t('config.hideDetails') : $t('config.viewDetails') }}</span>
              </button>
            </div>
          </div>
        </div>
      </div>
    </div>
  </div>
  <UpdateDialog />
</template>

<script setup>
import { ref, onMounted, computed } from 'vue'
import { getVersion } from '@tauri-apps/api/app'
import { open } from '@tauri-apps/plugin-shell'
import { useI18n } from 'vue-i18n'
import UpdateDialog from '@/components/UpdateDialog.vue'
import { useAutoUpdate } from '@/composables/useAutoUpdate'
import logger from '../../utils/logger'

const { t } = useI18n()

const appVersion = ref('0.0.0')
const githubUrl = 'https://github.com/JDBeWL/Mercurial-Player'
const showLicenseDetails = ref(false)

// 自动更新
const { isChecking, updateAvailable, newVersion, checkForUpdates, error, lastCheckTime, updateLog } = useAutoUpdate()

// 技术栈分类数据
const techCategories = computed(() => [
  {
    name: t('config.techCategoryCore'),
    icon: 'apps',
    techs: [
      {
        name: 'Tauri',
        desc: t('config.techTauri'),
        docs: 'https://tauri.app/',
        repo: 'https://github.com/tauri-apps/tauri'
      },
      {
        name: 'Vue 3',
        desc: t('config.techVue'),
        docs: 'https://vuejs.org/',
        repo: 'https://github.com/vuejs/core'
      }
    ]
  },
  {
    name: t('config.techCategoryAudio'),
    icon: 'graphic_eq',
    techs: [
      {
        name: 'Symphonia',
        desc: t('config.techSymphonia'),
        docs: 'https://docs.rs/symphonia/',
        repo: 'https://github.com/pdeljanov/Symphonia'
      },
      {
        name: 'Rodio',
        desc: t('config.techRodio'),
        docs: 'https://docs.rs/rodio/',
        repo: 'https://github.com/RustAudio/rodio'
      },
      {
        name: 'CPAL',
        desc: t('config.techCPAL'),
        docs: 'https://docs.rs/cpal/',
        repo: 'https://github.com/RustAudio/cpal'
      }
    ]
  }
])

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

const openLink = async (url) => {
  try {
    await open(url)
  } catch (error) {
    logger.error('Failed to open link:', error)
  }
}

const openLicense = async () => {
  try {
    // 打开 LICENSE 文件或 GitHub 上的许可证页面
    await open('https://www.gnu.org/licenses/gpl-3.0.html')
  } catch (error) {
    logger.error('Failed to open license:', error)
  }
}

const isDownloadFinishedLog = (s) => {
  return typeof s === 'string' && s.startsWith('Download finished:')
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
  background-color: var(--md-sys-color-surface-container);
  border-radius: 24px;
}

.app-header {
  display: flex;
  align-items: baseline;
  width: 100%;
  gap: 20px;
}

.app-name {
  font-size: 2.25rem;
  font-weight: 500;
  color: var(--md-sys-color-on-surface);
  line-height: 1.2;
}

.app-version {
  font-size: 14px;
  color: var(--md-sys-color-primary);
  white-space: nowrap;
  font-family: 'Courier New', Courier, monospace;
  font-variant-numeric: tabular-nums;
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
  transition: background-color 0.2s ease;
  position: relative;
  z-index: 0;
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
  cursor: pointer;
  padding: 4px;
  border-radius: 8px;
  transition: all 0.2s ease;
}

.link-arrow:hover {
  background-color: var(--md-sys-color-surface-container-high);
  color: var(--md-sys-color-primary);
}

.tech-categories {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(280px, 1fr));
  gap: 16px;
}

.tech-category-card {
  padding: 20px;
  background-color: var(--md-sys-color-surface-container);
  border-radius: 16px;
  display: flex;
  flex-direction: column;
  gap: 16px;
}

.category-header {
  display: flex;
  align-items: center;
  gap: 12px;
}

.category-icon {
  font-size: 24px;
  color: var(--md-sys-color-primary);
}

.category-title {
  font-size: 18px;
  font-weight: 500;
  color: var(--md-sys-color-on-surface);
  margin: 0;
}

.category-note {
  font-size: 12px;
  color: var(--md-sys-color-on-surface-variant);
  margin: 0;
  padding: 8px 12px;
  background-color: var(--md-sys-color-surface-container-high);
  border-radius: 8px;
  line-height: 1.5;
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
  gap: 12px;
  position: relative;
  z-index: 0;
}

.tech-info {
  flex: 1;
  display: flex;
  flex-direction: column;
  gap: 2px;
  min-width: 0;
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

.tech-links {
  display: flex;
  gap: 4px;
  flex-shrink: 0;
  position: relative;
  z-index: 1;
}

.tech-link-btn {
  width: 32px;
  height: 32px;
  padding: 0;
  display: flex;
  align-items: center;
  justify-content: center;
  background: none;
  border: none;
  border-radius: 8px;
  color: var(--md-sys-color-on-surface-variant);
  cursor: pointer;
  transition: all 0.2s ease;
  position: relative;
  z-index: 2;
}

.tech-link-btn:hover {
  background-color: var(--md-sys-color-surface-container-high);
  color: var(--md-sys-color-primary);
}

.tech-link-btn:hover .material-symbols-rounded {
  color: var(--md-sys-color-primary);
}

.tech-link-btn .material-symbols-rounded {
  font-size: 18px;
}

.license-card {
  padding: 24px;
  background-color: var(--md-sys-color-surface-container);
  border-radius: 16px;
  position: relative;
}

.license-external-link {
  position: absolute;
  top: 16px;
  right: 16px;
  width: 36px;
  height: 36px;
  display: flex;
  align-items: center;
  justify-content: center;
  background: none;
  border: none;
  border-radius: 8px;
  color: var(--md-sys-color-on-surface-variant);
  cursor: pointer;
  transition: all 0.2s ease;
  z-index: 1;
}

.license-external-link:hover {
  background-color: var(--md-sys-color-surface-container-high);
  color: var(--md-sys-color-primary);
}

.license-external-link .material-symbols-rounded {
  font-size: 20px;
}

.license-content {
  display: flex;
  gap: 24px;
  align-items: flex-start;
}

.license-icon {
  flex-shrink: 0;
  width: 80px;
  height: 80px;
  display: flex;
  align-items: center;
  justify-content: center;
  background-color: var(--md-sys-color-secondary-container);
  color: var(--md-sys-color-on-secondary-container);
  border-radius: 16px;
}

.license-icon .material-symbols-rounded {
  font-size: 48px;
}

.license-main {
  flex: 1;
  display: flex;
  flex-direction: column;
  gap: 8px;
  min-width: 0;
}

.license-title {
  font-size: 28px;
  font-weight: 600;
  color: var(--md-sys-color-on-surface);
  margin: 0;
  line-height: 1.2;
}

.license-subtitle {
  font-size: 14px;
  color: var(--md-sys-color-primary);
  margin: 0;
  font-weight: 500;
  text-transform: uppercase;
  letter-spacing: 0.5px;
}

.license-copyright {
  font-size: 12px;
  color: var(--md-sys-color-on-surface-variant);
  margin: 4px 0 0 0;
  font-weight: 400;
}

.license-legal-text {
  margin-top: 12px;
  padding-top: 16px;
  border-top: 1px solid var(--md-sys-color-outline-variant);
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.license-paragraph {
  font-size: 11px;
  line-height: 1.6;
  color: var(--md-sys-color-on-surface-variant);
  margin: 0;
  opacity: 0.7;
}

.license-details-toggle {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 8px 12px;
  margin-top: 8px;
  background: none;
  border: none;
  border-radius: 8px;
  cursor: pointer;
  font-size: 12px;
  color: var(--md-sys-color-primary);
  transition: all 0.2s ease;
  align-self: flex-start;
}

.license-details-toggle:hover {
  background-color: var(--md-sys-color-surface-container-high);
}

.license-details-toggle .material-symbols-rounded {
  font-size: 18px;
}
</style>
