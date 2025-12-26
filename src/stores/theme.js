import { defineStore } from 'pinia';
import {
  argbFromHex,
  hexFromArgb,
  themeFromSourceColor,
  applyTheme,
  TonalPalette,
  Hct,
} from '@material/material-color-utilities';
import logger from '../utils/logger';
import { validateThemeContrast } from '../utils/themeContrastValidator';

// 缓存已生成的主题样式
const customStyleCache = new Map();
let customStyleElement = null;

// 辅助函数：从 HEX 颜色生成色调变体
function generateTonalVariants(hexColor) {
  const argb = argbFromHex(hexColor);
  const hct = Hct.fromInt(argb);
  const palette = TonalPalette.fromHct(hct);
  
  const tones = [0, 5, 10, 15, 20, 25, 30, 35, 40, 50, 60, 70, 80, 90, 95, 98, 99, 100];
  const result = {};
  for (const tone of tones) {
    result[`tone${tone}`] = hexFromArgb(palette.tone(tone));
  }
  return result;
}

// 辅助函数：生成互补色和类似色
function generateHarmonyColors(hexColor) {
  const argb = argbFromHex(hexColor);
  const hct = Hct.fromInt(argb);
  const hue = hct.hue;
  const chroma = hct.chroma;
  const tone = hct.tone;
  
  return {
    complementary: hexFromArgb(Hct.from((hue + 180) % 360, chroma, tone).toInt()),
    analogous1: hexFromArgb(Hct.from((hue + 30) % 360, chroma, tone).toInt()),
    analogous2: hexFromArgb(Hct.from((hue + 330) % 360, chroma, tone).toInt()),
    triadic1: hexFromArgb(Hct.from((hue + 120) % 360, chroma, tone).toInt()),
    triadic2: hexFromArgb(Hct.from((hue + 240) % 360, chroma, tone).toInt()),
  };
}

// 生成自定义 CSS 变量（不包括 Material Design 基础颜色）
function generateCustomCSS(primaryColor, isDark, enableGlass, enableGradients) {
  const primaryHct = Hct.fromInt(argbFromHex(primaryColor));
  const isLightColor = primaryHct.tone > 50;
  const primaryPalette = TonalPalette.fromHct(primaryHct);
  
  // 计算 on-primary 和 container 颜色
  const onPrimaryColor = isLightColor ? '#000000' : '#ffffff';
  const containerTone = isDark ? 30 : 90;
  const onContainerTone = isDark ? 90 : 10;
  const containerColor = hexFromArgb(primaryPalette.tone(containerTone));
  const onContainerColor = hexFromArgb(primaryPalette.tone(onContainerTone));
  
  // 生成色调变体和和谐色
  const tones = generateTonalVariants(primaryColor);
  const harmony = generateHarmonyColors(primaryColor);
  const accentTones = generateTonalVariants(harmony.complementary);
  
  let css = '';
  
  // 覆盖 primary 相关颜色
  css += `--md-sys-color-primary: ${primaryColor};\n`;
  css += `--md-sys-color-on-primary: ${onPrimaryColor};\n`;
  css += `--md-sys-color-primary-container: ${containerColor};\n`;
  css += `--md-sys-color-on-primary-container: ${onContainerColor};\n`;
  
  // 主题源颜色
  css += `--theme-source-color: ${primaryColor};\n`;
  css += `--theme-on-primary: ${onPrimaryColor};\n`;
  css += `--theme-on-primary-container: ${onContainerColor};\n`;
  
  // 主色调变体
  for (const [key, value] of Object.entries(tones)) {
    css += `--theme-primary-${key}: ${value};\n`;
  }
  
  // 和谐色
  css += `--theme-complementary: ${harmony.complementary};\n`;
  css += `--theme-analogous-1: ${harmony.analogous1};\n`;
  css += `--theme-analogous-2: ${harmony.analogous2};\n`;
  css += `--theme-triadic-1: ${harmony.triadic1};\n`;
  css += `--theme-triadic-2: ${harmony.triadic2};\n`;
  
  // 强调色变体
  for (const [key, value] of Object.entries(accentTones)) {
    css += `--theme-accent-${key}: ${value};\n`;
  }
  
  // 阴影
  const shadowAlpha = isDark ? [0.5, 0.6, 0.7] : [0.08, 0.12, 0.16];
  css += `--shadow-soft: 0 4px 20px rgba(0, 0, 0, ${shadowAlpha[0]});\n`;
  css += `--shadow-medium: 0 8px 30px rgba(0, 0, 0, ${shadowAlpha[1]});\n`;
  css += `--shadow-strong: 0 12px 40px rgba(0, 0, 0, ${shadowAlpha[2]});\n`;
  
  // 主色透明度变体
  css += `--primary-alpha-5: color-mix(in srgb, ${primaryColor} 5%, transparent);\n`;
  css += `--primary-alpha-10: color-mix(in srgb, ${primaryColor} 10%, transparent);\n`;
  css += `--primary-alpha-20: color-mix(in srgb, ${primaryColor} 20%, transparent);\n`;
  css += `--primary-alpha-30: color-mix(in srgb, ${primaryColor} 30%, transparent);\n`;
  
  // 玻璃态效果
  if (enableGlass) {
    css += `--glass-blur: 12px;\n`;
    css += `--glass-opacity: ${isDark ? '0.75' : '0.85'};\n`;
    css += `--glass-border: 1px solid rgba(255, 255, 255, ${isDark ? '0.1' : '0.3'});\n`;
    css += `--glass-shadow: 0 8px 32px rgba(0, 0, 0, ${isDark ? '0.4' : '0.1'});\n`;
  } else {
    css += `--glass-blur: 0px;\n`;
    css += `--glass-opacity: 1;\n`;
    css += `--glass-border: none;\n`;
    css += `--glass-shadow: var(--md-sys-elevation-level2);\n`;
  }
  
  // 渐变效果
  if (enableGradients) {
    css += `--gradient-primary: linear-gradient(135deg, var(--md-sys-color-primary) 0%, var(--md-sys-color-primary) 100%);\n`;
    css += `--gradient-surface: none;\n`;
    css += `--gradient-accent: linear-gradient(135deg, var(--md-sys-color-primary) 0%, var(--md-sys-color-tertiary) 100%);\n`;
    css += `--gradient-background: none;\n`;
    css += `--gradient-hover: linear-gradient(135deg, var(--md-sys-color-primary) 0%, var(--md-sys-color-primary) 100%);\n`;
  } else {
    css += `--gradient-primary: none;\n`;
    css += `--gradient-surface: none;\n`;
    css += `--gradient-accent: none;\n`;
    css += `--gradient-background: none;\n`;
    css += `--gradient-hover: none;\n`;
  }
  
  return css;
}

export const useThemeStore = defineStore('theme', {
  state: () => ({
    isDarkMode: false,
    themePreference: 'auto',
    primaryColor: '#E67EA5',
    enableGlassEffect: true,
    enableGradients: true,
  }),

  getters: {
    isDark: (state) => state.isDarkMode,
  },

  actions: {
    async toggleDarkMode() {
      this.isDarkMode = !this.isDarkMode;
      if (this.themePreference !== 'auto') {
        this.themePreference = this.isDarkMode ? 'dark' : 'light';
      }
      this.applyTheme();
      await this.saveThemeToConfig();
    },

    async setPrimaryColor(color) {
      this.primaryColor = color;
      this.themePreference = color;
      this.applyTheme();
      await this.saveThemeToConfig();
    },

    setThemePreference(preference) {
      this.themePreference = preference;
      if (preference === 'auto') {
        this.isDarkMode = window.matchMedia('(prefers-color-scheme: dark)').matches;
        this.primaryColor = '#E67EA5';
      } else if (preference === 'light') {
        this.isDarkMode = false;
        this.primaryColor = '#E67EA5';
      } else if (preference === 'dark') {
        this.isDarkMode = true;
        this.primaryColor = '#E67EA5';
      } else if (preference.startsWith('#')) {
        this.primaryColor = preference;
      }
      this.applyTheme();
    },
    
    setGlassEffect(enabled) {
      this.enableGlassEffect = enabled;
      this.applyTheme();
    },
    
    setGradients(enabled) {
      this.enableGradients = enabled;
      this.applyTheme();
    },

    applyTheme() {
      // 1. 使用 Material Design 库设置基础颜色
      const theme = themeFromSourceColor(argbFromHex(this.primaryColor));
      applyTheme(theme, { target: document.documentElement, dark: this.isDarkMode });
      
      // 2. 生成并应用自定义 CSS（覆盖和扩展）
      const cacheKey = `${this.primaryColor}-${this.isDarkMode}-${this.enableGlassEffect}-${this.enableGradients}`;
      
      if (!customStyleCache.has(cacheKey)) {
        const customCSS = generateCustomCSS(
          this.primaryColor, 
          this.isDarkMode, 
          this.enableGlassEffect, 
          this.enableGradients
        );
        customStyleCache.set(cacheKey, customCSS);
        
        // 限制缓存大小
        if (customStyleCache.size > 20) {
          const firstKey = customStyleCache.keys().next().value;
          customStyleCache.delete(firstKey);
        }
      }
      
      // 创建或更新自定义 style 元素
      if (!customStyleElement) {
        customStyleElement = document.createElement('style');
        customStyleElement.id = 'theme-custom-variables';
        document.head.appendChild(customStyleElement);
      }
      
      customStyleElement.textContent = `:root {\n${customStyleCache.get(cacheKey)}}`;
      
      // 设置 data-theme 属性
      document.documentElement.setAttribute('data-theme', this.isDarkMode ? 'dark' : 'light');
      
      logger.debug('Theme applied:', cacheKey);
      
      // 验证颜色对比度
      setTimeout(() => {
        const results = validateThemeContrast(this.isDarkMode);
        if (results.failed.length > 0) {
          logger.warn('主题颜色对比度不符合 WCAG 标准:', results.failed);
        }
      }, 50);
    },
    
    async saveThemeToConfig() {
      try {
        const { useConfigStore } = await import('./config');
        const configStore = useConfigStore();
        configStore.general.theme = this.themePreference;
        await configStore.saveConfigNow();
      } catch (error) {
        logger.error('Failed to save theme to config:', error);
      }
    },
  },
});
