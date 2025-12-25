/**
 * 颜色对比度工具
 * 用于检查颜色是否符合 WCAG 无障碍标准
 */

/**
 * 将十六进制颜色转换为 RGB
 * @param {string} hex - 十六进制颜色值（如 #RRGGBB 或 #RGB）
 * @returns {Object} { r, g, b } RGB 值（0-255）
 */
function hexToRgb(hex) {
  // 移除 # 符号
  hex = hex.replace('#', '');
  
  // 处理 3 位十六进制颜色
  if (hex.length === 3) {
    hex = hex.split('').map(char => char + char).join('');
  }
  
  const r = parseInt(hex.substring(0, 2), 16);
  const g = parseInt(hex.substring(2, 4), 16);
  const b = parseInt(hex.substring(4, 6), 16);
  
  return { r, g, b };
}

/**
 * 计算相对亮度（根据 WCAG 标准）
 * @param {number} r - 红色分量 (0-255)
 * @param {number} g - 绿色分量 (0-255)
 * @param {number} b - 蓝色分量 (0-255)
 * @returns {number} 相对亮度 (0-1)
 */
function getRelativeLuminance(r, g, b) {
  // 转换为 0-1 范围
  const [rs, gs, bs] = [r, g, b].map(val => {
    val = val / 255;
    return val <= 0.03928 ? val / 12.92 : Math.pow((val + 0.055) / 1.055, 2.4);
  });
  
  return 0.2126 * rs + 0.7152 * gs + 0.0722 * bs;
}

/**
 * 计算两个颜色之间的对比度
 * @param {string} color1 - 第一个颜色（十六进制）
 * @param {string} color2 - 第二个颜色（十六进制）
 * @returns {number} 对比度比值
 */
export function getContrastRatio(color1, color2) {
  const rgb1 = hexToRgb(color1);
  const rgb2 = hexToRgb(color2);
  
  const lum1 = getRelativeLuminance(rgb1.r, rgb1.g, rgb1.b);
  const lum2 = getRelativeLuminance(rgb2.r, rgb2.g, rgb2.b);
  
  const lighter = Math.max(lum1, lum2);
  const darker = Math.min(lum1, lum2);
  
  return (lighter + 0.05) / (darker + 0.05);
}

/**
 * 检查颜色对比度是否符合 WCAG 标准
 * @param {string} foreground - 前景色（十六进制）
 * @param {string} background - 背景色（十六进制）
 * @param {Object} options - 选项
 * @param {string} options.level - WCAG 级别 ('AA' 或 'AAA')
 * @param {boolean} options.largeText - 是否为大文本（≥18pt 或 ≥14pt 粗体）
 * @returns {Object} { pass: boolean, ratio: number, level: string, message: string }
 */
export function checkContrast(foreground, background, options = {}) {
  const { level = 'AA', largeText = false } = options;
  
  const ratio = getContrastRatio(foreground, background);
  
  // WCAG 标准
  const standards = {
    AA: {
      normal: 4.5,
      large: 3.0,
    },
    AAA: {
      normal: 7.0,
      large: 4.5,
    },
  };
  
  const requiredRatio = largeText 
    ? standards[level].large 
    : standards[level].normal;
  
  const pass = ratio >= requiredRatio;
  
  const message = pass
    ? `符合 WCAG ${level} 标准（对比度 ${ratio.toFixed(2)}:1，要求 ${requiredRatio}:1）`
    : `不符合 WCAG ${level} 标准（对比度 ${ratio.toFixed(2)}:1，要求 ${requiredRatio}:1）`;
  
  return {
    pass,
    ratio: parseFloat(ratio.toFixed(2)),
    requiredRatio,
    level,
    largeText,
    message,
  };
}

/**
 * 调整颜色亮度以确保符合对比度要求
 * @param {string} color - 原始颜色（十六进制）
 * @param {string} targetBackground - 目标背景色（十六进制）
 * @param {Object} options - 选项
 * @param {string} options.level - WCAG 级别
 * @param {boolean} options.largeText - 是否为大文本
 * @param {number} options.step - 调整步长（0-1）
 * @returns {string} 调整后的颜色（十六进制）
 */
export function adjustColorForContrast(color, targetBackground, options = {}) {
  const { level = 'AA', largeText = false, step = 0.05 } = options;
  
  let currentColor = color;
  let check = checkContrast(currentColor, targetBackground, { level, largeText });
  
  // 如果已经符合要求，直接返回
  if (check.pass) {
    return currentColor;
  }
  
  const rgb = hexToRgb(currentColor);
  const bgRgb = hexToRgb(targetBackground);
  const bgLum = getRelativeLuminance(bgRgb.r, bgRgb.g, bgRgb.b);
  
  // 确定需要变亮还是变暗
  const currentLum = getRelativeLuminance(rgb.r, rgb.g, rgb.b);
  const shouldLighten = currentLum < bgLum;
  
  // 调整颜色
  let attempts = 0;
  const maxAttempts = 100;
  
  while (!check.pass && attempts < maxAttempts) {
    if (shouldLighten) {
      // 变亮
      rgb.r = Math.min(255, Math.round(rgb.r + (255 - rgb.r) * step));
      rgb.g = Math.min(255, Math.round(rgb.g + (255 - rgb.g) * step));
      rgb.b = Math.min(255, Math.round(rgb.b + (255 - rgb.b) * step));
    } else {
      // 变暗
      rgb.r = Math.max(0, Math.round(rgb.r * (1 - step)));
      rgb.g = Math.max(0, Math.round(rgb.g * (1 - step)));
      rgb.b = Math.max(0, Math.round(rgb.b * (1 - step)));
    }
    
    currentColor = `#${[rgb.r, rgb.g, rgb.b]
      .map(val => val.toString(16).padStart(2, '0'))
      .join('')}`;
    
    check = checkContrast(currentColor, targetBackground, { level, largeText });
    attempts++;
  }
  
  return currentColor;
}

/**
 * 从 CSS 变量获取颜色值
 * @param {string} varName - CSS 变量名（如 '--md-sys-color-primary'）
 * @returns {string|null} 颜色值或 null
 */
export function getColorFromCSSVar(varName) {
  if (typeof window === 'undefined') return null;
  
  const value = getComputedStyle(document.documentElement)
    .getPropertyValue(varName)
    .trim();
  
  if (!value) return null;
  
  // 如果是 rgb/rgba，转换为十六进制
  if (value.startsWith('rgb')) {
    const matches = value.match(/\d+/g);
    if (matches && matches.length >= 3) {
      const r = parseInt(matches[0]);
      const g = parseInt(matches[1]);
      const b = parseInt(matches[2]);
      return `#${[r, g, b].map(val => val.toString(16).padStart(2, '0')).join('')}`;
    }
  }
  
  return value.startsWith('#') ? value : null;
}

/**
 * 批量检查颜色组合
 * @param {Array} colorPairs - 颜色对数组 [{ foreground, background, name, largeText? }]
 * @param {string} level - WCAG 级别
 * @returns {Array} 检查结果数组
 */
export function checkColorPairs(colorPairs, level = 'AA') {
  return colorPairs.map(({ foreground, background, name, largeText = false }) => {
    const check = checkContrast(foreground, background, { level, largeText });
    return {
      name,
      foreground,
      background,
      ...check,
    };
  });
}

/**
 * WCAG 标准常量
 */
export const WCAG_STANDARDS = {
  AA: {
    normal: 4.5,
    large: 3.0,
  },
  AAA: {
    normal: 7.0,
    large: 4.5,
  },
};

