# 状态/告警/错误提示音预览资产

该目录用于维护本地试听预览资产（不直接接入固件运行时资产）。

## 目录结构

- `scores/*.json`：音效源分数（可重复生成输入）
- `audio/*.mid`：MIDI 预览文件
- `audio/*.wav`：WAV 试听文件
- `cues.manifest.json`：提示音清单与循环语义契约
- `generation-report.json`：批量生成后的数量与分类校验摘要
- `preview.html`：本地增强预览页面

## 生成方式

在仓库根目录执行：

```bash
python3 tools/audio/gen_status_alert_error_previews.py
```

默认使用仓库内置工具 `tools/audio/buzzer_preview.py` 生成 `MIDI + WAV`，也可通过 `--buzzer-tool` 覆盖。

## 本地预览

在仓库根目录执行：

```bash
python3 -m http.server -d docs 8000
```

然后打开：

- `http://127.0.0.1:8000/audio-cues-preview/preview.html`
