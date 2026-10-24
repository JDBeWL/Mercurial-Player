# 节拍分析组件

`models/` 和 `runtime/` 不入库，用脚本下载到本目录：

```bash
pnpm fetch:beat
```

- `models/` 必装；`runtime/` 只有 Windows GPU 用，没有就自动退回 rten CPU。
- 已存在且 SHA-256 正确的文件会跳过；`pnpm fetch:beat --force` 强制重下，
  `pnpm fetch:beat --models-only` 只拉模型。
- 固定版本：Beat This! small `v1.1.0`（danigb/beat-this-rs）、
  ONNX Runtime DirectML `1.24.4`（NuGet）。

## 开发时让应用找到它

模型：

```bash
export MERCURIAL_MODEL_DIR="$PWD/plugins/beat-analysis/models"
```

GPU 运行时（可选，运行时的探测路径没有环境变量）：

```bash
mkdir -p src-tauri/resources/runtime
cp plugins/beat-analysis/runtime/*.dll src-tauri/resources/runtime/
```

不复制也能跑，只是退回 rten CPU（慢一个数量级，结果一致）。PowerShell 的环境变量写法是
`$env:MERCURIAL_MODEL_DIR = "$PWD/plugins/beat-analysis/models"`。

## 安装/打包

把 `models/` 和 `runtime/` 放到 `<安装目录>/plugins/beat-analysis/`。应用插件页的
「打开目录」会直接建好这两个目录，用户把文件拖进去即可。
