// 颜色预设类型
export interface ColorPreset {
  hex: string
  name: string
  category: string
}

// 精选颜色预设（带名称和分类）- 使用用户直观可见的颜色
export const colorPresets: ColorPreset[] = [
  // 蓝色系 - 清新、专业、信任
  { hex: '#64B5F6', name: '天空蓝', category: 'blue' },
  { hex: '#42A5F5', name: '晴空蓝', category: 'blue' },
  { hex: '#0288D1', name: '月村手毬', category: 'blue' },
  { hex: '#2196F3', name: '经典蓝', category: 'blue' },
  { hex: '#1E88E5', name: '海洋蓝', category: 'blue' },
  { hex: '#1976D2', name: '深海蓝', category: 'blue' },
  { hex: '#0097A7', name: '青绿蓝', category: 'blue' },
  { hex: '#00ACC1', name: '水鸭蓝', category: 'blue' },
  { hex: '#00BCD4', name: '青色', category: 'blue' },
  { hex: '#26C6DA', name: '浅青色', category: 'blue' },

  // 紫色系 - 神秘、优雅、创意
  { hex: '#7C4DFF', name: '电光紫', category: 'purple' },
  { hex: '#651FFF', name: '深紫', category: 'purple' },
  { hex: '#6200EA', name: '靛紫', category: 'purple' },
  { hex: '#7E57C2', name: '薰衣草紫', category: 'purple' },
  { hex: '#673AB7', name: '经典紫', category: 'purple' },
  { hex: '#5E35B1', name: '深薰衣草', category: 'purple' },
  { hex: '#512DA8', name: '皇家紫', category: 'purple' },
  { hex: '#536DFE', name: '靛蓝紫', category: 'purple' },
  { hex: '#3D5AFE', name: '明亮靛蓝', category: 'purple' },
  { hex: '#304FFE', name: '深靛蓝', category: 'purple' },

  // 粉色系 - 浪漫、温柔、活力
  { hex: '#E67EA5', name: '杏山和纱', category: 'pink' },
  { hex: '#F48FB1', name: '樱花粉', category: 'pink' },
  { hex: '#F06292', name: '玫瑰粉', category: 'pink' },
  { hex: '#EC407A', name: '亮粉', category: 'pink' },
  { hex: '#E91E63', name: '经典粉', category: 'pink' },
  { hex: '#D81B60', name: '深玫瑰', category: 'pink' },
  { hex: '#C2185B', name: '酒红粉', category: 'pink' },
  { hex: '#AD1457', name: '深酒红', category: 'pink' },
  { hex: '#FF4081', name: '霓虹粉', category: 'pink' },
  { hex: '#F50057', name: '亮玫红', category: 'pink' },
  { hex: '#E040FB', name: '紫粉', category: 'pink' },

  // 红色系 - 热情、活力、警示
  { hex: '#EF5350', name: '珊瑚红', category: 'red' },
  { hex: '#F44336', name: '经典红', category: 'red' },
  { hex: '#E53935', name: '鲜红', category: 'red' },
  { hex: '#D32F2F', name: '深红', category: 'red' },
  { hex: '#C62828', name: '暗红', category: 'red' },
  { hex: '#B71C1C', name: '酒红', category: 'red' },
  { hex: '#FF5252', name: '亮红', category: 'red' },
  { hex: '#FF1744', name: '霓虹红', category: 'red' },
  { hex: '#D50000', name: '纯红', category: 'red' },

  // 橙色系 - 温暖、活泼、创意
  { hex: '#FF7043', name: '珊瑚橙', category: 'orange' },
  { hex: '#FF5722', name: '深橙', category: 'orange' },
  { hex: '#F4511E', name: '烈焰橙', category: 'orange' },
  { hex: '#E64A19', name: '暗橙', category: 'orange' },
  { hex: '#FF9800', name: '经典橙', category: 'orange' },
  { hex: '#FB8C00', name: '南瓜橙', category: 'orange' },
  { hex: '#F57C00', name: '深南瓜', category: 'orange' },
  { hex: '#FFA726', name: '杏橙', category: 'orange' },
  { hex: '#FFB300', name: '琥珀', category: 'orange' },
  { hex: '#FFC107', name: '金黄', category: 'orange' },

  // 绿色系 - 自然、健康、成长
  { hex: '#66BB6A', name: '草绿', category: 'green' },
  { hex: '#4CAF50', name: '经典绿', category: 'green' },
  { hex: '#43A047', name: '森林绿', category: 'green' },
  { hex: '#388E3C', name: '深森林', category: 'green' },
  { hex: '#2E7D32', name: '暗绿', category: 'green' },
  { hex: '#00E676', name: '霓虹绿', category: 'green' },
  { hex: '#00C853', name: '亮绿', category: 'green' },
  { hex: '#009688', name: '青绿', category: 'green' },
  { hex: '#00897B', name: '深青绿', category: 'green' },
  { hex: '#26A69A', name: '薄荷绿', category: 'green' },

  // 莫奈系 - 印象派光影，取自莫奈代表画作
  { hex: '#4A7A7C', name: '睡莲青', category: 'monet' },
  { hex: '#B878A0', name: '睡莲粉', category: 'monet' },
  { hex: '#C97757', name: '印象橙', category: 'monet' },
  { hex: '#5470A0', name: '印象蓝', category: 'monet' },
  { hex: '#B08840', name: '干草金', category: 'monet' },
  { hex: '#7E6492', name: '黄昏紫', category: 'monet' },
  { hex: '#BC8569', name: '教堂蜜', category: 'monet' },
  { hex: '#5E5285', name: '鸢尾紫', category: 'monet' },
  { hex: '#6B7B8A', name: '雾灰蓝', category: 'monet' },
  { hex: '#4E7E5E', name: '水面绿', category: 'monet' },

  // 中性色 - 稳重、专业、简约
  { hex: '#2C2C2C', name: '鬼方佳代子', category: 'neutral' },
  { hex: '#78909C', name: '蓝灰', category: 'neutral' },
  { hex: '#607D8B', name: '深蓝灰', category: 'neutral' },
  { hex: '#546E7A', name: '暗蓝灰', category: 'neutral' },
  { hex: '#455A64', name: '炭灰', category: 'neutral' },
  { hex: '#37474F', name: '深炭灰', category: 'neutral' },
  { hex: '#795548', name: '棕色', category: 'neutral' },
  { hex: '#6D4C41', name: '深棕', category: 'neutral' },
  { hex: '#5D4037', name: '咖啡棕', category: 'neutral' },
  { hex: '#8D6E63', name: '浅棕', category: 'neutral' },
  { hex: '#9E9E9E', name: '中灰', category: 'neutral' },
]
