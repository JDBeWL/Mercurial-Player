// 同步版本号: 从 src-tauri/Cargo.toml 读取 version,写入 package.json 和 tauri.conf.json
//
// 用法: pnpm run version:sync
//
// 工作流:
//   1. 编辑 src-tauri/Cargo.toml 的 version 字段 (唯一来源)
//   2. 运行 pnpm run version:sync
//   3. 提交所有变更 (Cargo.toml + package.json + tauri.conf.json)

import { readFileSync, writeFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { dirname, join } from 'node:path'

const __dirname = dirname(fileURLToPath(import.meta.url))
const root = join(__dirname, '..')

// --- 读取 Cargo.toml 中的版本号 ---
const cargoPath = join(root, 'src-tauri', 'Cargo.toml')
const cargoContent = readFileSync(cargoPath, 'utf8')

// 匹配 [package] 段下的 version = "x.y.z"
// 只取 [package] 之后第一次出现的 version,避免 [dependencies] 段中的版本字段干扰
const packageSectionStart = cargoContent.indexOf('[package]')
if (packageSectionStart === -1) {
  console.error('[version:sync] Cargo.toml 缺少 [package] 段')
  process.exit(1)
}

const afterPackage = cargoContent.slice(packageSectionStart)
// 截断到下一个 [xxx] 段之前
const nextSectionMatch = afterPackage.slice(1).match(/\n\[/)
const packageSection = nextSectionMatch
  ? afterPackage.slice(0, 1 + nextSectionMatch.index)
  : afterPackage

const versionMatch = packageSection.match(/^version\s*=\s*"([^"]+)"/m)
if (!versionMatch) {
  console.error('[version:sync] Cargo.toml [package] 段缺少 version 字段')
  process.exit(1)
}

const version = versionMatch[1]
console.log(`[version:sync] Cargo.toml version = ${version}`)

// --- 更新 package.json ---
const pkgPath = join(root, 'package.json')
const pkg = JSON.parse(readFileSync(pkgPath, 'utf8'))
if (pkg.version !== version) {
  pkg.version = version
  writeFileSync(pkgPath, JSON.stringify(pkg, null, 2) + '\n', 'utf8')
  console.log(`[version:sync] package.json: ${pkg.version} -> ${version}`)
} else {
  console.log(`[version:sync] package.json: 已是 ${version},跳过`)
}

// --- 更新 tauri.conf.json ---
const tauriConfPath = join(root, 'src-tauri', 'tauri.conf.json')
const tauriConf = JSON.parse(readFileSync(tauriConfPath, 'utf8'))
if (tauriConf.version !== version) {
  tauriConf.version = version
  writeFileSync(tauriConfPath, JSON.stringify(tauriConf, null, 2) + '\n', 'utf8')
  console.log(`[version:sync] tauri.conf.json: ${tauriConf.version} -> ${version}`)
} else {
  console.log(`[version:sync] tauri.conf.json: 已是 ${version},跳过`)
}

console.log('[version:sync] 完成')
