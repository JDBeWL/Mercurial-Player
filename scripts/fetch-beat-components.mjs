// 下载可选的节拍分析组件到 plugins/beat-analysis/。
// 版本与 SHA-256 固定；--force 强制重下，--models-only 跳过 GPU 运行时。
import { createHash } from 'node:crypto'
import { spawnSync } from 'node:child_process'
import { mkdirSync } from 'node:fs'
import { mkdir, mkdtemp, readFile, rename, rm, writeFile } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

const ROOT = join(dirname(fileURLToPath(import.meta.url)), '..')
const TARGET = join(ROOT, 'plugins', 'beat-analysis')
const FORCE = process.argv.includes('--force')
const MODELS_ONLY = process.argv.includes('--models-only') || process.platform !== 'win32'

const MODEL_TAG = 'v1.1.0'
const MODELS = [
  {
    name: 'beat_this_small.onnx',
    url: `https://raw.githubusercontent.com/danigb/beat-this-rs/${MODEL_TAG}/models/beat_this_small.onnx`,
    sha256: 'a5f8d39d989f31859454ba27afe61c5317ca95e4d9373e6853e5361b8937172f',
  },
  {
    name: 'mel_spectrogram.onnx',
    url: `https://raw.githubusercontent.com/danigb/beat-this-rs/${MODEL_TAG}/models/mel_spectrogram.onnx`,
    sha256: 'fdd59e65c515331308e4c8841edf99972deca646bdf6197744c2a5b7755e3de9',
  },
]

const RUNTIME = {
  package: 'microsoft.ml.onnxruntime.directml',
  version: '1.24.4',
  files: {
    'onnxruntime.dll': 'e7eedec6a6f26dc39dc948276a75ef6d2bee3fff944d874ceed0bbd3b97bff40',
    'onnxruntime_providers_shared.dll':
      '265c8daf29637cb259cac8be9f08f2cd45f3883f0f0e4949cbfddd5b4cbec3b6',
  },
}

const sha256 = (data) => createHash('sha256').update(data).digest('hex')

async function matches(path, expected) {
  try {
    return sha256(await readFile(path)) === expected
  } catch {
    return false
  }
}

async function fetchBuffer(url) {
  const response = await fetch(url, {
    headers: { 'user-agent': 'mercurial-player-fetch/1.0' },
  })
  if (!response.ok) throw new Error(`HTTP ${response.status}: ${url}`)
  return Buffer.from(await response.arrayBuffer())
}

async function installFile(dest, data, expected, label) {
  const actual = sha256(data)
  if (actual !== expected) {
    throw new Error(`${label} SHA-256 不匹配\n  期望 ${expected}\n  实际 ${actual}`)
  }
  await mkdir(dirname(dest), { recursive: true })
  const temp = `${dest}.part`
  await writeFile(temp, data)
  await rename(temp, dest)
}

function extractZip(zip, dest) {
  mkdirSync(dest, { recursive: true })
  const quote = (value) => value.replaceAll("'", "''")
  const attempts = [
    ['unzip', ['-o', '-q', zip, '-d', dest]],
    ['tar', ['-xf', zip, '-C', dest]],
    [
      'powershell',
      [
        '-NoProfile',
        '-NonInteractive',
        '-Command',
        `Expand-Archive -LiteralPath '${quote(zip)}' -DestinationPath '${quote(dest)}' -Force`,
      ],
    ],
  ]
  for (const [command, args] of attempts) {
    if (spawnSync(command, args, { stdio: 'ignore' }).status === 0) return
  }
  throw new Error('解压 nupkg 失败（需要 unzip、tar 或 PowerShell）')
}

async function installModels() {
  for (const model of MODELS) {
    const dest = join(TARGET, 'models', model.name)
    if (!FORCE && (await matches(dest, model.sha256))) {
      console.log(`✓ models/${model.name}（已存在）`)
      continue
    }
    console.log(`↓ models/${model.name}`)
    await installFile(dest, await fetchBuffer(model.url), model.sha256, `models/${model.name}`)
    console.log(`✓ models/${model.name}`)
  }
}

async function installRuntime() {
  if (MODELS_ONLY) {
    console.log('· 跳过 GPU 运行时（非 Windows 或 --models-only）')
    return
  }

  const pending = []
  for (const [name, expected] of Object.entries(RUNTIME.files)) {
    if (!FORCE && (await matches(join(TARGET, 'runtime', name), expected))) {
      console.log(`✓ runtime/${name}（已存在）`)
    } else {
      pending.push([name, expected])
    }
  }
  if (pending.length === 0) return

  const nupkg = `${RUNTIME.package}.${RUNTIME.version}.nupkg`
  const url = `https://api.nuget.org/v3-flatcontainer/${RUNTIME.package}/${RUNTIME.version}/${nupkg}`
  const temp = await mkdtemp(join(tmpdir(), 'mercurial-ort-'))
  try {
    console.log(`↓ ${RUNTIME.package} ${RUNTIME.version}`)
    const zip = join(temp, nupkg)
    await writeFile(zip, await fetchBuffer(url))
    const extracted = join(temp, 'x')
    extractZip(zip, extracted)
    for (const [name, expected] of pending) {
      const source = join(extracted, 'runtimes', 'win-x64', 'native', name)
      await installFile(
        join(TARGET, 'runtime', name),
        await readFile(source),
        expected,
        `runtime/${name}`,
      )
      console.log(`✓ runtime/${name}`)
    }
  } finally {
    await rm(temp, { recursive: true, force: true })
  }
}

try {
  await installModels()
  await installRuntime()
  console.log(`\n完成：${TARGET}`)
} catch (error) {
  console.error(`\n下载失败：${error.message}`)
  process.exitCode = 1
}
