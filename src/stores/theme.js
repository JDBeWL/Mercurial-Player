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

// 辅助函数：从 HEX 颜色生成色调变体
function generateTonalVariants(hexColor) {
  const argb = argbFromHex(hexColor);
  const hct = Hct.fromInt(argb);
  const palette = TonalPalette.fromHct(hct);
  
  return {
    tone0: hexFromArgb(palette.tone(0)),
    tone5: hexFromArgb(palette.tone(5)),
    tone10: hexFromArgb(palette.tone(10)),
    tone15: hexFromArgb(palette.tone(15)),
    tone20: hexFromArgb(palette.tone(20)),
    tone25: hexFromArgb(palette.tone(25)),
    tone30: hexFromArgb(palette.tone(30)),
    tone35: hexFromArgb(palette.tone(35)),
    tone40: hexFromArgb(palette.tone(40)),
    tone50: hexFromArgb(palette.tone(50)),
    tone60: hexFromArgb(palette.tone(60)),
    tone70: hexFromArgb(palette.tone(70)),
    tone80: hexFromArgb(palette.tone(80)),
    tone90: hexFromArgb(palette.tone(90)),
    tone95: hexFromArgb(palette.tone(95)),
    tone98: hexFromArgb(palette.tone(98)),
    tone99: hexFromArgb(palette.tone(99)),
    tone100: hexFromArgb(palette.tone(100)),
  };
}

// 辅助函数：生成互补色和类似色
function generateHarmonyColors(hexColor) {
  const argb = argbFromHex(hexColor);
  const hct = Hct.fromInt(argb);
  const hue = hct.hue;
  const chroma = hct.chroma;
  const tone = hct.tone;
  
  // 互补色 (180度)
  const complementary = Hct.from((hue + 180) % 360, chroma, tone);
  // 类似色 (±30度)
  const analogous1 = Hct.from((hue + 30) % 360, chroma, tone);
  const analogous2 = Hct.from((hue + 330) % 360, chroma, tone);
  // 三角色 (±120度)
  const triadic1 = Hct.from((hue + 120) % 360, chroma, tone);
  const triadic2 = Hct.from((hue + 240) % 360, chroma, tone);
  
  return {
    complementary: hexFromArgb(complementary.toInt()),
    analogous1: hexFromArgb(analogous1.toInt()),
    analogous2: hexFromArgb(analogous2.toInt()),
    triadic1: hexFromArgb(triadic1.toInt()),
    triadic2: hexFromArgb(triadic2.toInt()),
  };
}

export const useThemeStore = defineStore('theme', {
  state: () => ({
    isDarkMode: false,
    themePreference: 'auto', // 'auto', 'light', 'dark', or a hex color
    primaryColor: '#E67EA5', // 默认杏山和纱主题色
    enableGlassEffect: true, // 玻璃态效果
    enableGradients: true, // 渐变效果
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
        this.primaryColor = '#E67EA5'; // 默认杏山和纱主题色
      } else if (preference === 'light') {
        this.isDarkMode = false;
        this.primaryColor = '#E67EA5'; // 默认杏山和纱主题色
      } else if (preference === 'dark') {
        this.isDarkMode = true;
        this.primaryColor = '#E67EA5'; // 默认杏山和纱主题色
      } else if (preference.startsWith('#')) {
        this.primaryColor = preference;
      }
      this.applyTheme();
    },
    
    setGlassEffect(enabled) {
      this.enableGlassEffect = enabled;
      this.applyEnhancedStyles();
    },
    
    setGradients(enabled) {
      this.enableGradients = enabled;
      this.applyEnhancedStyles();
    },

    applyTheme() {
      const theme = themeFromSourceColor(argbFromHex(this.primaryColor));
      applyTheme(theme, { target: document.documentElement, dark: this.isDarkMode });
      document.documentElement.setAttribute('data-theme', this.isDarkMode ? 'dark' : 'light');
      
      // 强制使用用户选择的颜色作为主色，覆盖 Material Design 生成的颜色
      document.documentElement.style.setProperty('--md-sys-color-primary', this.primaryColor);
      
      // 根据主色亮度计算 on-primary 颜色
      const primaryArgb = argbFromHex(this.primaryColor);
      const primaryHct = Hct.fromInt(primaryArgb);
      const isLightColor = primaryHct.tone > 50;
      
      // on-primary: 浅色主色用深色文字，深色主色用浅色文字
      const onPrimaryColor = isLightColor ? '#000000' : '#ffffff';
      document.documentElement.style.setProperty('--md-sys-color-on-primary', onPrimaryColor);
      document.documentElement.style.setProperty('--theme-on-primary', onPrimaryColor);
      
      // 生成 primary-container 颜色（主色的浅色/深色变体）
      const containerTone = this.isDarkMode ? 30 : 90;
      const onContainerTone = this.isDarkMode ? 90 : 10;
      const primaryPalette = TonalPalette.fromHct(primaryHct);
      const containerColor = hexFromArgb(primaryPalette.tone(containerTone));
      const onContainerColor = hexFromArgb(primaryPalette.tone(onContainerTone));
      
      document.documentElement.style.setProperty('--md-sys-color-primary-container', containerColor);
      document.documentElement.style.setProperty('--md-sys-color-on-primary-container', onContainerColor);
      document.documentElement.style.setProperty('--theme-on-primary-container', onContainerColor);
      
      // 保存源颜色供其他用途
      document.documentElement.style.setProperty('--theme-source-color', this.primaryColor);
      
      // 生成并应用增强的色调变体
      this.applyTonalVariants();
      
      // 应用增强样式（渐变、玻璃态等）
      this.applyEnhancedStyles();
      
      logger.debug('Generated Theme:', theme);
      logger.debug('Applied primary color:', this.primaryColor);
      
      // 验证颜色对比度（延迟执行以确保 CSS 变量已更新）
      setTimeout(() => {
        const results = validateThemeContrast(this.isDarkMode);
        // 只对关键失败项进行警告
        if (results.failed.length > 0) {
          logger.warn('主题颜色对比度不符合 WCAG 标准（关键组合）:', results.failed);
        }
        // 设计权衡的警告在开发模式下显示
        if (results.warnings.length > 0 && import.meta.env.DEV) {
          logger.debug('主题颜色对比度设计权衡:', results.warnings);
        }
      }, 100);
    },
    
    applyTonalVariants() {
      const root = document.documentElement;
      const tones = generateTonalVariants(this.primaryColor);
      const harmony = generateHarmonyColors(this.primaryColor);
      
      // 设置主色调变体
      Object.entries(tones).forEach(([key, value]) => {
        root.style.setProperty(`--theme-primary-${key}`, value);
      });
      
      // 设置和谐色
      root.style.setProperty('--theme-complementary', harmony.complementary);
      root.style.setProperty('--theme-analogous-1', harmony.analogous1);
      root.style.setProperty('--theme-analogous-2', harmony.analogous2);
      root.style.setProperty('--theme-triadic-1', harmony.triadic1);
      root.style.setProperty('--theme-triadic-2', harmony.triadic2);
      
      // 生成互补色的色调变体（用于强调色）
      const complementaryTones = generateTonalVariants(harmony.complementary);
      Object.entries(complementaryTones).forEach(([key, value]) => {
        root.style.setProperty(`--theme-accent-${key}`, value);
      });
    },
    
    applyEnhancedStyles() {
      const root = document.documentElement;
      const isDark = this.isDarkMode;
      
      // 玻璃态效果变量
      if (this.enableGlassEffect) {
        root.style.setProperty('--glass-blur', '12px');
        root.style.setProperty('--glass-opacity', isDark ? '0.75' : '0.85');
        root.style.setProperty('--glass-border', isDark 
          ? '1px solid rgba(255, 255, 255, 0.1)' 
          : '1px solid rgba(255, 255, 255, 0.3)');
        root.style.setProperty('--glass-shadow', isDark
          ? '0 8px 32px rgba(0, 0, 0, 0.4)'
          : '0 8px 32px rgba(0, 0, 0, 0.1)');
      } else {
        root.style.setProperty('--glass-blur', '0px');
        root.style.setProperty('--glass-opacity', '1');
        root.style.setProperty('--glass-border', 'none');
        root.style.setProperty('--glass-shadow', 'var(--md-sys-elevation-level2)');
      }
      
      // 渐变效果 - 使用 CSS 变量引用 Material Design 生成的颜色
      if (this.enableGradients) {
        // 主渐变（用于按钮、强调元素）- 使用 MD3 的 primary 颜色
        root.style.setProperty('--gradient-primary', 
          `linear-gradient(135deg, var(--md-sys-color-primary) 0%, var(--md-sys-color-primary) 100%)`);
        
        // 表面渐变（用于卡片、容器背景）
        root.style.setProperty('--gradient-surface', 'none');
        
        // 强调渐变
        root.style.setProperty('--gradient-accent', 
          `linear-gradient(135deg, var(--md-sys-color-primary) 0%, var(--md-sys-color-tertiary) 100%)`);
        
        // 背景渐变 - 禁用，保持纯色
        root.style.setProperty('--gradient-background', 'none');
          
        // 悬浮渐变
        root.style.setProperty('--gradient-hover', 
          `linear-gradient(135deg, var(--md-sys-color-primary) 0%, var(--md-sys-color-primary) 100%)`);
      } else {
        root.style.setProperty('--gradient-primary', 'none');
        root.style.setProperty('--gradient-surface', 'none');
        root.style.setProperty('--gradient-accent', 'none');
        root.style.setProperty('--gradient-background', 'none');
        root.style.setProperty('--gradient-hover', 'none');
      }
      
      // 增强的阴影效果
      root.style.setProperty('--shadow-soft', isDark
        ? '0 4px 20px rgba(0, 0, 0, 0.5)'
        : '0 4px 20px rgba(0, 0, 0, 0.08)');
      root.style.setProperty('--shadow-medium', isDark
        ? '0 8px 30px rgba(0, 0, 0, 0.6)'
        : '0 8px 30px rgba(0, 0, 0, 0.12)');
      root.style.setProperty('--shadow-strong', isDark
        ? '0 12px 40px rgba(0, 0, 0, 0.7)'
        : '0 12px 40px rgba(0, 0, 0, 0.16)');
        
      // 主色调透明度变体（用于背景叠加）
      root.style.setProperty('--primary-alpha-5', `color-mix(in srgb, var(--md-sys-color-primary) 5%, transparent)`);
      root.style.setProperty('--primary-alpha-10', `color-mix(in srgb, var(--md-sys-color-primary) 10%, transparent)`);
      root.style.setProperty('--primary-alpha-20', `color-mix(in srgb, var(--md-sys-color-primary) 20%, transparent)`);
      root.style.setProperty('--primary-alpha-30', `color-mix(in srgb, var(--md-sys-color-primary) 30%, transparent)`);
    },
    
    async saveThemeToConfig() {
      try {
        const { useConfigStore } = await import('./config');
        const configStore = useConfigStore();
        // 保存主题偏好（可能是 'light', 'dark', 'auto' 或颜色值）
        configStore.general.theme = this.themePreference;
        await configStore.saveConfigNow();
      } catch (error) {
        logger.error('Failed to save theme to config:', error);
      }
    },
  },
});