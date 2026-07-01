#!/usr/bin/env python3
"""Generate the selected interaction feedback tones and preview page."""

from __future__ import annotations

import json
import subprocess
import sys
from dataclasses import dataclass
from pathlib import Path
from tempfile import TemporaryDirectory


ROOT = Path(__file__).resolve().parents[2]
OUT_DIR = ROOT / "docs" / "audio-cues-preview" / "interaction-feedback"
SCORES_DIR = OUT_DIR / "scores"
AUDIO_DIR = OUT_DIR / "audio"
FIRMWARE_AUDIO_DIR = ROOT / "firmware" / "assets" / "audio" / "interaction-cues"
BUZZER_TOOL = ROOT / "tools" / "audio" / "buzzer_preview.py"
SELECTED_SET_ID = "set_b_warm_tap"
PREVIEW_SAMPLE_RATE_HZ = 8_000
FIRMWARE_SAMPLE_RATE_HZ = 44_100


@dataclass(frozen=True)
class Tone:
    tone_id: str
    title: str
    intent: str
    waveform: str
    volume: float
    fade_ms: int
    events: list[dict[str, object]]


@dataclass(frozen=True)
class SetDef:
    set_id: str
    title: str
    character: str
    recommendation: str
    touch: Tone
    usb_c: Tone


def freq(freq_hz: float, ms: int, velocity: int | None = None) -> dict[str, object]:
    event: dict[str, object] = {"freq_hz": freq_hz, "ms": ms}
    if velocity is not None:
        event["velocity"] = velocity
    return event


def rest(ms: int) -> dict[str, object]:
    return {"rest_ms": ms}


def tone(tone_id: str, title: str, intent: str, waveform: str, volume: float, fade_ms: int, events: list[dict[str, object]]) -> Tone:
    return Tone(
        tone_id=tone_id,
        title=title,
        intent=intent,
        waveform=waveform,
        volume=volume,
        fade_ms=fade_ms,
        events=events,
    )


def sets() -> list[SetDef]:
    return [
        SetDef(
            set_id="set_a_soft_tick",
            title="A. Soft Tick",
            character="Very short, low-visibility tactile click. USB-C is a clean two-step rise.",
            recommendation="Safest default when touch/key feedback should stay present but quiet.",
            touch=tone(
                "set_a_touch",
                "Touch / Button",
                "valid touch, valid key",
                "sine",
                0.28,
                5,
                [freq(1680, 22), rest(8), freq(1260, 18)],
            ),
            usb_c=tone(
                "set_a_usb_c_insert",
                "USB-C Insert",
                "USB-C inserted",
                "sine",
                0.42,
                7,
                [freq(780, 55), rest(22), freq(1170, 72), rest(18), freq(1560, 60)],
            ),
        ),
        SetDef(
            set_id="set_b_warm_tap",
            title="B. Warm Tap",
            character="Softer rounded touch with a warmer mid tone. USB-C is a confident low-to-high connect chime.",
            recommendation="Best if the enclosure/speaker makes high tones too sharp.",
            touch=tone(
                "set_b_touch",
                "Touch / Button",
                "valid touch, valid key",
                "sine",
                0.30,
                6,
                [freq(1120, 30), rest(10), freq(1480, 22)],
            ),
            usb_c=tone(
                "set_b_usb_c_insert",
                "USB-C Insert",
                "USB-C inserted",
                "sine",
                0.46,
                8,
                [freq(520, 64), rest(20), freq(920, 64), rest(20), freq(1380, 86)],
            ),
        ),
        SetDef(
            set_id="set_c_crisp_micro",
            title="C. Crisp Micro",
            character="Sharper and more mechanical, still below alert territory. USB-C uses a distinct bright arpeggio.",
            recommendation="Best if the front panel needs more tactile confirmation in a noisy room.",
            touch=tone(
                "set_c_touch",
                "Touch / Button",
                "valid touch, valid key",
                "square",
                0.18,
                3,
                [freq(2100, 16), rest(6), freq(1700, 14)],
            ),
            usb_c=tone(
                "set_c_usb_c_insert",
                "USB-C Insert",
                "USB-C inserted",
                "square",
                0.28,
                5,
                [freq(880, 38), rest(16), freq(1320, 46), rest(16), freq(1760, 58), rest(20), freq(1320, 42)],
            ),
        ),
        SetDef(
            set_id="set_d_muted_blip",
            title="D. Muted Blip",
            character="Most understated touch; almost a soft UI blip. USB-C is lower, longer, and clearly not a tap.",
            recommendation="Best if repeated touches may happen often and fatigue is the main risk.",
            touch=tone(
                "set_d_touch",
                "Touch / Button",
                "valid touch, valid key",
                "sine",
                0.24,
                7,
                [freq(940, 26), rest(7), freq(1210, 16)],
            ),
            usb_c=tone(
                "set_d_usb_c_insert",
                "USB-C Insert",
                "USB-C inserted",
                "sine",
                0.40,
                10,
                [freq(660, 76), rest(24), freq(990, 96), rest(24), freq(660, 52), rest(18), freq(1320, 70)],
            ),
        ),
    ]


def score_for(t: Tone, sample_rate_hz: int) -> dict[str, object]:
    return {
        "tempo_bpm": 240,
        "ppqn": 480,
        "audio": {
            "sample_rate_hz": sample_rate_hz,
            "waveform": t.waveform,
            "volume": t.volume,
            "fade_ms": t.fade_ms,
        },
        "events": t.events,
    }


def write_preview_html(manifest: dict[str, object]) -> None:
    data = json.dumps(manifest, ensure_ascii=False, indent=2)
    html = f"""<!doctype html>
<html lang="zh-CN">
  <head>
    <meta charset="UTF-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1.0" />
    <title>B. Warm Tap 交互操作音预览</title>
    <style>
      :root {{
        --bg: #f7f9fb;
        --panel: #ffffff;
        --ink: #17202a;
        --muted: #5c6977;
        --line: #d9e1ea;
        --accent: #0b6f78;
        --accent-soft: #e6f5f6;
        --usb: #8a4f0f;
        --touch: #236b42;
        --danger: #a73545;
      }}
      * {{ box-sizing: border-box; }}
      body {{
        margin: 0;
        min-height: 100vh;
        font-family: "Noto Sans SC", "Source Han Sans SC", "PingFang SC", system-ui, sans-serif;
        color: var(--ink);
        background: linear-gradient(180deg, #eef5f8 0, var(--bg) 280px);
      }}
      main {{
        width: min(1120px, calc(100vw - 32px));
        margin: 0 auto;
        padding: 28px 0 42px;
        display: grid;
        gap: 16px;
      }}
      header {{
        display: grid;
        gap: 8px;
      }}
      h1 {{
        margin: 0;
        font-size: clamp(1.45rem, 3vw, 2.15rem);
        line-height: 1.15;
        letter-spacing: 0;
      }}
      p {{ margin: 0; color: var(--muted); line-height: 1.55; }}
      .toolbar, .set {{
        background: var(--panel);
        border: 1px solid var(--line);
        border-radius: 8px;
        box-shadow: 0 10px 24px rgba(31, 45, 61, 0.08);
      }}
      .toolbar {{
        padding: 12px;
        display: grid;
        gap: 12px;
        grid-template-columns: 1fr auto;
        align-items: center;
      }}
      .volume {{
        display: flex;
        align-items: center;
        gap: 10px;
        min-width: 0;
      }}
      .volume input {{ width: min(340px, 48vw); }}
      .value {{
        min-width: 3.2em;
        color: var(--accent);
        font-variant-numeric: tabular-nums;
        font-weight: 700;
      }}
      button, a {{
        min-height: 36px;
        border: 1px solid var(--line);
        border-radius: 8px;
        padding: 7px 11px;
        color: var(--ink);
        background: #fff;
        font: inherit;
        text-decoration: none;
        cursor: pointer;
      }}
      button:hover, a:hover {{ filter: brightness(0.98); }}
      button:active {{ transform: translateY(1px); }}
      .stop-all {{
        color: #fff;
        background: var(--danger);
        border-color: transparent;
      }}
      .sets {{
        display: grid;
        gap: 14px;
      }}
      .set {{
        padding: 14px;
        display: grid;
        gap: 14px;
      }}
      .set-head {{
        display: grid;
        gap: 7px;
      }}
      .set-head h2 {{
        margin: 0;
        font-size: 1.18rem;
        letter-spacing: 0;
      }}
      .recommendation {{
        color: var(--accent);
        font-weight: 650;
      }}
      .tones {{
        display: grid;
        gap: 10px;
        grid-template-columns: repeat(auto-fit, minmax(260px, 1fr));
      }}
      .tone {{
        border: 1px solid var(--line);
        border-radius: 8px;
        padding: 12px;
        display: grid;
        gap: 10px;
        background: #fff;
      }}
      .tone.playing {{
        outline: 3px solid rgba(11, 111, 120, 0.15);
        border-color: var(--accent);
      }}
      .tone-title {{
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: 10px;
      }}
      .tone-title h3 {{
        margin: 0;
        font-size: 1rem;
      }}
      .badge {{
        border-radius: 999px;
        padding: 4px 8px;
        font-size: 0.78rem;
        font-weight: 700;
        white-space: nowrap;
      }}
      .badge.touch {{ color: var(--touch); background: #eaf5ee; }}
      .badge.usb {{ color: var(--usb); background: #fff0dd; }}
      .meta {{
        color: var(--muted);
        font-size: 0.9rem;
        font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace;
      }}
      .actions {{
        display: flex;
        flex-wrap: wrap;
        gap: 8px;
        align-items: center;
      }}
      .play {{ background: var(--accent-soft); border-color: #b9dde1; }}
      .download {{ color: var(--accent); }}
      @media (max-width: 660px) {{
        main {{ width: min(100vw - 20px, 1120px); padding-top: 18px; }}
        .toolbar {{ grid-template-columns: 1fr; }}
        .volume {{ display: grid; }}
        .volume input {{ width: 100%; }}
      }}
    </style>
  </head>
  <body>
    <main>
      <header>
        <h1>B. Warm Tap 交互操作音预览</h1>
        <p>已选定的 touch/button 共用操作音与明显不同的 USB-C 插入音。预览 WAV 为 8kHz / mono / PCM16LE，固件资产同步生成为 44.1kHz / mono / PCM16LE。</p>
      </header>

      <section class="toolbar" aria-label="试听控制">
        <label class="volume">
          <span>预览音量</span>
          <input id="volume" type="range" min="0" max="1" step="0.01" value="0.75" />
          <span id="volume-value" class="value">0.75</span>
        </label>
        <button id="stop-all" class="stop-all">停止全部</button>
      </section>

      <section id="sets" class="sets"></section>
    </main>

    <script type="application/json" id="manifest-data">{data}</script>
    <script>
      const manifest = JSON.parse(document.getElementById("manifest-data").textContent);
      const setsEl = document.getElementById("sets");
      const volumeEl = document.getElementById("volume");
      const volumeValueEl = document.getElementById("volume-value");
      const stopAllEl = document.getElementById("stop-all");
      const players = new Map();

      function stopAll() {{
        for (const audio of players.values()) {{
          audio.pause();
          audio.currentTime = 0;
        }}
        document.querySelectorAll(".tone.playing").forEach((el) => el.classList.remove("playing"));
      }}

      function playerFor(tone) {{
        if (!players.has(tone.id)) {{
          const audio = new Audio(tone.wav);
          audio.preload = "auto";
          audio.volume = Number(volumeEl.value);
          audio.addEventListener("ended", () => {{
            document.querySelector(`[data-tone-id="${{tone.id}}"]`)?.classList.remove("playing");
          }});
          players.set(tone.id, audio);
        }}
        return players.get(tone.id);
      }}

      function renderTone(tone) {{
        const isUsb = tone.intent.includes("USB-C");
        const article = document.createElement("article");
        article.className = "tone";
        article.dataset.toneId = tone.id;
        article.innerHTML = `
          <div class="tone-title">
            <h3>${{tone.title}}</h3>
            <span class="badge ${{isUsb ? "usb" : "touch"}}">${{isUsb ? "USB-C" : "touch/key"}}</span>
          </div>
          <p>${{tone.intent}}</p>
          <div class="meta">${{tone.waveform}} · ${{tone.duration_ms}} ms · volume ${{tone.volume}}</div>
          <div class="actions">
            <button class="play" type="button">播放</button>
            <button class="stop" type="button">停止</button>
            <a class="download" href="${{tone.wav}}" target="_blank" rel="noreferrer">WAV</a>
            <a class="download" href="${{tone.mid}}" target="_blank" rel="noreferrer">MIDI</a>
          </div>
        `;
        article.querySelector(".play").addEventListener("click", async () => {{
          stopAll();
          const audio = playerFor(tone);
          audio.volume = Number(volumeEl.value);
          article.classList.add("playing");
          await audio.play();
        }});
        article.querySelector(".stop").addEventListener("click", () => {{
          const audio = playerFor(tone);
          audio.pause();
          audio.currentTime = 0;
          article.classList.remove("playing");
        }});
        return article;
      }}

      for (const set of manifest.sets) {{
        const section = document.createElement("article");
        section.className = "set";
        const head = document.createElement("div");
        head.className = "set-head";
        head.innerHTML = `
          <h2>${{set.title}}</h2>
          <p>${{set.character}}</p>
          <p class="recommendation">${{set.recommendation}}</p>
        `;
        const tones = document.createElement("div");
        tones.className = "tones";
        tones.append(renderTone(set.touch), renderTone(set.usb_c));
        section.append(head, tones);
        setsEl.append(section);
      }}

      volumeEl.addEventListener("input", () => {{
        const value = Number(volumeEl.value);
        volumeValueEl.textContent = value.toFixed(2);
        for (const audio of players.values()) audio.volume = value;
      }});
      stopAllEl.addEventListener("click", stopAll);
    </script>
  </body>
</html>
"""
    (OUT_DIR / "preview.html").write_text(html, encoding="utf-8")


def duration_ms(events: list[dict[str, object]]) -> int:
    total = 0
    for event in events:
        total += int(event.get("ms", event.get("rest_ms", 0)))
    return total


def main() -> int:
    OUT_DIR.mkdir(parents=True, exist_ok=True)
    SCORES_DIR.mkdir(parents=True, exist_ok=True)
    AUDIO_DIR.mkdir(parents=True, exist_ok=True)
    FIRMWARE_AUDIO_DIR.mkdir(parents=True, exist_ok=True)

    manifest_sets: list[dict[str, object]] = []
    for set_def in sets():
        if set_def.set_id != SELECTED_SET_ID:
            continue
        tones: dict[str, dict[str, object]] = {}
        for key, tone_def in (("touch", set_def.touch), ("usb_c", set_def.usb_c)):
            score_path = SCORES_DIR / f"{tone_def.tone_id}.json"
            wav_path = AUDIO_DIR / f"{tone_def.tone_id}.wav"
            mid_path = AUDIO_DIR / f"{tone_def.tone_id}.mid"
            score_path.write_text(
                json.dumps(score_for(tone_def, PREVIEW_SAMPLE_RATE_HZ), ensure_ascii=False, indent=2) + "\n",
                encoding="utf-8",
            )
            with TemporaryDirectory() as tmp:
                subprocess.run(
                    [
                        sys.executable,
                        str(BUZZER_TOOL),
                        "--in",
                        str(score_path),
                        "--out-dir",
                        tmp,
                    ],
                    check=True,
                    cwd=ROOT,
                )
                generated_wav = Path(tmp) / f"{score_path.stem}.wav"
                generated_mid = Path(tmp) / f"{score_path.stem}.mid"
                wav_path.write_bytes(generated_wav.read_bytes())
                mid_path.write_bytes(generated_mid.read_bytes())
            firmware_score_path = SCORES_DIR / f"{tone_def.tone_id}.firmware.json"
            firmware_score_path.write_text(
                json.dumps(score_for(tone_def, FIRMWARE_SAMPLE_RATE_HZ), ensure_ascii=False, indent=2) + "\n",
                encoding="utf-8",
            )
            with TemporaryDirectory() as tmp:
                subprocess.run(
                    [
                        sys.executable,
                        str(BUZZER_TOOL),
                        "--in",
                        str(firmware_score_path),
                        "--out-dir",
                        tmp,
                    ],
                    check=True,
                    cwd=ROOT,
                )
                firmware_name = "interaction_touch.wav" if key == "touch" else "usb_c_insert.wav"
                (FIRMWARE_AUDIO_DIR / firmware_name).write_bytes(
                    (Path(tmp) / f"{firmware_score_path.stem}.wav").read_bytes()
                )
            tones[key] = {
                "id": tone_def.tone_id,
                "title": tone_def.title,
                "intent": tone_def.intent,
                "waveform": tone_def.waveform,
                "volume": tone_def.volume,
                "duration_ms": duration_ms(tone_def.events),
                "wav": f"./audio/{tone_def.tone_id}.wav",
                "mid": f"./audio/{tone_def.tone_id}.mid",
            }
        manifest_sets.append(
            {
                "id": set_def.set_id,
                "title": set_def.title,
                "character": set_def.character,
                "recommendation": set_def.recommendation,
                "touch": tones["touch"],
                "usb_c": tones["usb_c"],
            }
        )

    manifest = {
        "title": "Selected interaction feedback tones",
        "selected_set_id": SELECTED_SET_ID,
        "sample_rate_hz": PREVIEW_SAMPLE_RATE_HZ,
        "firmware_sample_rate_hz": FIRMWARE_SAMPLE_RATE_HZ,
        "format": "WAV PCM16LE mono",
        "sets": manifest_sets,
    }
    (OUT_DIR / "manifest.json").write_text(json.dumps(manifest, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    write_preview_html(manifest)
    print(f"Wrote {OUT_DIR / 'preview.html'}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
