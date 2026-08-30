/**
 * 歌词字体注册表
 *
 * 字体来源分三类，统一按同一套文件名约定解析字体族名与字重：
 *   - 内置字体：npm 包引入，随应用分发（如霞鹜文楷屏幕版）
 *   - 打包字体：`src/assets/fonts/lyrics/` 下的字体文件，构建期打包
 *     （dev 模式下新增/删除文件需要重启 dev server，glob 是构建期解析的）
 *   - 外部字体：软件同级 `fonts/` 目录下的字体文件，运行时动态扫描加载，
 *     放入文件后无需重启应用，重新打开设置页即可看到。
 *     TTC/OTC 集合文件（一个文件含多个字体）由后端自动提取成员为
 *     单字体缓存文件，成员按字体内部名称生成（如 `族名-700.ttf`），
 *     对前端而言与普通文件无异
 *
 * 文件名约定（打包字体与外部字体）：
 *   - `字体名.ttf`     → 常规字重（400）
 *   - `字体名-700.ttf` → 指定字重（100~900，同一字体族可有多个字重文件）
 *   - `字体名-VF.otf`  → 可变字体，注册为 100~900 全字重区间（推荐，一个文件覆盖全部字重）
 *   - `字体名-Bold.otf` → 英文权重名后缀（Adobe / Google Fonts 官方包命名，见 WEIGHT_NAMES）
 * 支持格式：ttf / otf / woff / woff2
 */
import { ref } from 'vue'
import { convertFileSrc, invoke } from '@tauri-apps/api/core'
import logger from './logger'

const fontModules = import.meta.glob<string>('../assets/fonts/lyrics/*.{ttf,otf,woff,woff2}', {
  eager: true,
  query: '?url',
  import: 'default',
})

interface FontFaceRule {
  family: string
  /** 字重值或可变字体的字重区间（如 "100 900"） */
  weight: number | string
  url: string
  format: string
}

const FORMAT_BY_EXT: Record<string, string> = {
  ttf: 'truetype',
  otf: 'opentype',
  woff: 'woff',
  woff2: 'woff2',
}

/**
 * 常见英文权重名 → 数值字重，兼容 Adobe（SourceHanSansSC-Bold.otf）
 * 与 Google Fonts（Poppins-SemiBold.ttf）官方包的命名。
 * 长名称排前面，保证 ExtraLight 先于 Light、SemiBold 先于 Bold 匹配
 */
const WEIGHT_NAMES: [name: string, weight: number][] = [
  ['extralight', 200],
  ['ultralight', 200],
  ['semibold', 600],
  ['demibold', 600],
  ['extrabold', 800],
  ['ultrabold', 800],
  ['thin', 100],
  ['light', 300],
  ['regular', 400],
  ['normal', 400],
  ['book', 400],
  ['medium', 500],
  ['bold', 700],
  ['black', 900],
  ['heavy', 900],
]

/** 按文件名约定解析字体族名与字重 */
export function parseFontFileName(file: string): {
  family: string
  /* 字重值或可变字体的字重区间（如 "100 900"） */
  weight: number | string
  format: string
} {
  const ext = (file.split('.').pop() ?? '').toLowerCase()
  const stem = file.slice(0, -(ext.length + 1))
  const weightMatch = stem.match(/-(\d{3})$/)
  // `-VF` 后缀表示可变字体，注册为全字重区间，一个文件覆盖全部字重
  const variableMatch = /-VF$/i.test(stem)
  let rawFamily = stem
  let weight: number | string = 400
  if (weightMatch) {
    rawFamily = stem.slice(0, -4)
    weight = Number(weightMatch[1])
  } else {
    const lower = stem.toLowerCase()
    const hit = WEIGHT_NAMES.find(([name]) => lower.endsWith(`-${name}`))
    if (hit) {
      rawFamily = stem.slice(0, stem.length - hit[0].length - 1)
      weight = hit[1]
    }
  }
  // 字体族名中不允许出现引号，直接剔除避免破坏生成的 @font-face
  const family = (variableMatch ? rawFamily.slice(0, -3) : rawFamily).replace(/['"]/g, '')
  return {
    family,
    weight: variableMatch ? '100 900' : weight,
    format: FORMAT_BY_EXT[ext] ?? 'truetype',
  }
}

function toFontFaceCss(f: FontFaceRule): string {
  // 可变字体优先使用系统已安装的同名族（零加载），未安装再加载打包/外部文件。
  // 固定字重的规则不加 local()：按族名匹配会命中错误字重的面，
  // 导致声明的 font-weight 与实际轮廓不符
  const src =
    typeof f.weight === 'string'
      ? `local('${f.family}'), url('${f.url}') format('${f.format}')`
      : `url('${f.url}') format('${f.format}')`
  return `@font-face{font-family:'${f.family}';src:${src};font-weight:${f.weight};font-style:normal;font-display:swap;}`
}

/* 将一组 @font-face 规则注入（或替换）指定 id 的样式元素 */
function applyStyleElement(id: string, css: string): void {
  if (typeof document === 'undefined') return
  document.getElementById(id)?.remove()
  const style = document.createElement('style')
  style.id = id
  style.textContent = css
  document.head.appendChild(style)
}

// ===== 打包字体（构建期） =====

const bundledFaces: FontFaceRule[] = Object.entries(fontModules).map(([path, url]) => ({
  ...parseFontFileName(path.split('/').pop() ?? path),
  url,
}))

if (bundledFaces.length > 0) {
  applyStyleElement('bundled-font-faces', bundledFaces.map(toFontFaceCss).join('\n'))
}

/* 通过 npm 包引入、随应用分发的内置字体 */
const PRESET_FONTS: { value: string; label: string }[] = [
  { value: 'LXGW WenKai Screen', label: '霞鹜文楷屏幕版 LXGW WenKai Screen' },
]

/* 手动放入 assets/fonts/lyrics/ 的字体（按文件名生成字体族名） */
const dropInFonts: { value: string; label: string }[] = [
  ...new Set(bundledFaces.map((f) => f.family)),
].map((family) => ({ value: family, label: family }))

/* 全部内置字体选项，供字体选择器分组展示 */
export const bundledFontOptions: { value: string; label: string }[] = [
  ...PRESET_FONTS,
  ...dropInFonts,
]

// ===== 外部字体（软件同级 fonts/ 目录，运行时动态加载） =====

interface FontOption {
  value: string
  label: string
}

/* 外部字体选项（响应式：loadExternalFonts 后自动更新选择器） */
export const externalFontOptions = ref<FontOption[]>([])

/**
 * 扫描软件同级 fonts/ 目录并注册其中全部字体。
 * 可重复调用：每次全量重建 @font-face 与选项列表，运行中放入新字体后调用即可生效。
 */
export async function loadExternalFonts(): Promise<void> {
  let fonts: { name: string; path: string }[]
  try {
    fonts = await invoke<{ name: string; path: string }[]>('get_external_fonts')
  } catch (error) {
    // 非 Tauri 环境（纯浏览器 dev）没有该命令，静默降级为无外部字体
    logger.debug('External fonts unavailable:', error)
    externalFontOptions.value = []
    return
  }

  const faces: FontFaceRule[] = fonts.map((f) => ({
    ...parseFontFileName(f.name),
    url: convertFileSrc(f.path),
  }))

  if (faces.length > 0) {
    applyStyleElement('external-font-faces', faces.map(toFontFaceCss).join('\n'))
  } else if (typeof document !== 'undefined') {
    document.getElementById('external-font-faces')?.remove()
  }
  externalFontOptions.value = [...new Set(faces.map((f) => f.family))].map((family) => ({
    value: family,
    label: family,
  }))
}
