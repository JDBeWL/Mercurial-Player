import { defineStore } from 'pinia';
import {
  argbFromHex,
  themeFromSourceColor,
  applyTheme,
} from '@material/material-color-utilities';

export const useThemeStore = defineStore('theme', {
  state: () => ({
    isDarkMode: false,
    themePreference: 'auto', // 'auto', 'light', 'dark', or a hex color
    primaryColor: '#64B5F6', // Default light blue
  }),

  actions: {
    async toggleDarkMode() {
      this.isDarkMode = !this.isDarkMode;
      document.documentElement.setAttribute('data-theme', this.isDarkMode ? 'dark' : 'light');
      if (this.themePreference !== 'auto') {
        this.themePreference = this.isDarkMode ? 'dark' : 'light';
      }
      this.applyTheme();
      
      // 保存主题设置到配置
      await this.saveThemeToConfig();
    },

    async setPrimaryColor(color) {
      this.primaryColor = color;
      this.themePreference = color; // 设置为自定义颜色
      this.applyTheme();
      
      // 保存主题设置到配置
      await this.saveThemeToConfig();
    },

    setThemePreference(preference) {
      this.themePreference = preference;
      if (preference === 'auto') {
        const prefersDarkMode = window.matchMedia('(prefers-color-scheme: dark)').matches;
        this.isDarkMode = prefersDarkMode;
        this.primaryColor = '#64B5F6'; // 重置为默认值
      } else if (preference === 'light') {
        this.isDarkMode = false;
        this.primaryColor = '#64B5F6'; // 重置为默认值
      } else if (preference === 'dark') {
        this.isDarkMode = true;
        this.primaryColor = '#64B5F6'; // 重置为默认值
      } else if (preference.startsWith('#')) {
        this.primaryColor = preference;
      }
      this.applyTheme();
    },

    applyTheme() {
      const theme = themeFromSourceColor(argbFromHex(this.primaryColor));
      applyTheme(theme, { target: document.documentElement, dark: this.isDarkMode });
      document.documentElement.setAttribute('data-theme', this.isDarkMode ? 'dark' : 'light');
      // 设置MD3基础颜色
      document.documentElement.style.setProperty('--md-sys-color-primary', this.primaryColor);
      console.log('Generated Theme:', theme);
    },
    
    async saveThemeToConfig() {
      try {
        const { useConfigStore } = await import('./config');
        const configStore = useConfigStore();
        // 保存主题偏好（可能是 'light', 'dark', 'auto' 或颜色值）
        configStore.general.theme = this.themePreference;
        await configStore.saveConfigNow();
      } catch (error) {
        console.error('Failed to save theme to config:', error);
      }
    },
  },
});