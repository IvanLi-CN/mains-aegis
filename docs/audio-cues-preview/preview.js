const MANIFEST_URL = "./cues.manifest.json";

const groupTitles = {
  status: "状态音",
  warning: "告警音",
  error: "错误音",
};

const activePlayers = new Map();
const activeErrorMixers = new Map();
const mixBufferCache = new Map();
let manifest = null;
let manifestBaseUrl = new URL(MANIFEST_URL, window.location.href).toString();
let mixAudioContext = null;
let errorBeatSeconds = 0.5;
let errorBarSeconds = 2.0;
let errorTransportOrigin = null;

const filterEl = document.getElementById("filter");
const volumeEl = document.getElementById("volume");
const volumeValueEl = document.getElementById("volume-value");
const warningIntervalEl = document.getElementById("warning-interval");
const stopAllEl = document.getElementById("stop-all");

const sections = {
  status: document.querySelector('#status-group .cue-list'),
  warning: document.querySelector('#warning-group .cue-list'),
  error: document.querySelector('#error-group .cue-list'),
};

const template = document.getElementById("cue-template");

function getGlobalVolume() {
  return Number.parseFloat(volumeEl.value || "0.85");
}

function clampInt(value, min, max) {
  return Math.min(max, Math.max(min, value));
}

function getWarningLoopInterval(defaultInterval) {
  const min = Number.parseInt(warningIntervalEl.min || "400", 10);
  const max = Number.parseInt(warningIntervalEl.max || "10000", 10);
  const fallback = clampInt(defaultInterval, min, max);
  const raw = Number.parseInt(warningIntervalEl.value || "", 10);
  if (!Number.isFinite(raw)) {
    warningIntervalEl.value = String(fallback);
    return fallback;
  }
  const clamped = clampInt(raw, min, max);
  if (clamped !== raw) {
    warningIntervalEl.value = String(clamped);
  }
  return clamped;
}

function cueItemKey(item) {
  return `${item.category}:${item.id}`;
}

function resolveAssetUrl(relativePath) {
  return new URL(relativePath, manifestBaseUrl).toString();
}

function setRowPlaying(item, playing) {
  const row = document.querySelector(`[data-cue-key="${cueItemKey(item)}"]`);
  if (!row) return;
  row.classList.toggle("playing", playing);
}

function refreshRowPlaying(item) {
  const key = cueItemKey(item);
  setRowPlaying(item, activePlayers.has(key) || activeErrorMixers.has(key));
}

function getErrorMixGainScale(count) {
  if (count <= 1) return 1.0;
  if (count === 2) return 0.68;
  return 0.55;
}

function syncErrorMixerGains() {
  const gainValue = getGlobalVolume() * getErrorMixGainScale(activeErrorMixers.size);
  for (const state of activeErrorMixers.values()) {
    state.gain.gain.value = gainValue;
  }
}

async function ensureMixAudioContext() {
  if (mixAudioContext) return mixAudioContext;
  const Ctx = window.AudioContext || window.webkitAudioContext;
  if (!Ctx) {
    throw new Error("当前浏览器不支持 WebAudio");
  }
  mixAudioContext = new Ctx();
  return mixAudioContext;
}

async function loadMixBuffer(item) {
  const key = cueItemKey(item);
  if (mixBufferCache.has(key)) {
    return mixBufferCache.get(key);
  }
  const ctx = await ensureMixAudioContext();
  const response = await fetch(resolveAssetUrl(item.wav_path));
  if (!response.ok) {
    throw new Error(`无法加载音频: ${item.id}`);
  }
  const data = await response.arrayBuffer();
  const buffer = await ctx.decodeAudioData(data);
  mixBufferCache.set(key, buffer);
  return buffer;
}

function nextAlignedErrorStart(ctx) {
  const lookAhead = 0.03;
  const minTime = ctx.currentTime + lookAhead;
  if (errorTransportOrigin === null) {
    const firstStart = Math.ceil(minTime / errorBeatSeconds) * errorBeatSeconds;
    errorTransportOrigin = firstStart;
    return firstStart;
  }
  if (minTime <= errorTransportOrigin) {
    return errorTransportOrigin;
  }
  const barsAhead = Math.ceil((minTime - errorTransportOrigin) / errorBarSeconds);
  return errorTransportOrigin + barsAhead * errorBarSeconds;
}

function stopErrorMix(item) {
  const key = cueItemKey(item);
  const state = activeErrorMixers.get(key);
  if (!state) return;

  try {
    state.source.stop();
  } catch {
    // Source may already be stopped.
  }
  state.source.disconnect();
  state.gain.disconnect();
  activeErrorMixers.delete(key);
  if (activeErrorMixers.size === 0) {
    errorTransportOrigin = null;
  }
  syncErrorMixerGains();
  refreshRowPlaying(item);
}

function stopAllErrorMixes() {
  if (!manifest) return;
  for (const item of manifest.items) {
    if (item.category === "error") {
      stopErrorMix(item);
    }
  }
}

async function playErrorMixLoop(item) {
  stopCue(item);
  stopErrorMix(item);

  const ctx = await ensureMixAudioContext();
  await ctx.resume();
  const buffer = await loadMixBuffer(item);
  const source = ctx.createBufferSource();
  const gain = ctx.createGain();
  source.buffer = buffer;
  source.loop = true;
  source.connect(gain);
  gain.connect(ctx.destination);

  const key = cueItemKey(item);
  source.onended = () => {
    const current = activeErrorMixers.get(key);
    if (!current || current.source !== source) return;
    activeErrorMixers.delete(key);
    syncErrorMixerGains();
    refreshRowPlaying(item);
  };

  activeErrorMixers.set(key, { source, gain });
  syncErrorMixerGains();
  refreshRowPlaying(item);
  source.start(nextAlignedErrorStart(ctx));
}

function stopCue(item) {
  const key = cueItemKey(item);
  const state = activePlayers.get(key);
  if (!state) return;

  state.stopped = true;
  if (state.timerId) {
    clearTimeout(state.timerId);
  }
  if (state.audio) {
    state.audio.pause();
    state.audio.currentTime = 0;
  }
  activePlayers.delete(key);
  refreshRowPlaying(item);
}

function stopAllCues() {
  if (!manifest) return;
  for (const item of manifest.items) {
    stopCue(item);
  }
  stopAllErrorMixes();
}

async function playOnce(item) {
  stopErrorMix(item);
  stopCue(item);
  const key = cueItemKey(item);
  const audio = new Audio(resolveAssetUrl(item.wav_path));
  audio.volume = getGlobalVolume();

  const state = {
    audio,
    timerId: 0,
    stopped: false,
    mode: "once",
  };
  activePlayers.set(key, state);
  setRowPlaying(item, true);

  audio.addEventListener("ended", () => {
    const latest = activePlayers.get(key);
    if (!latest || latest.mode !== "once") return;
    activePlayers.delete(key);
    refreshRowPlaying(item);
  });

  audio.addEventListener("error", () => {
    stopCue(item);
  });

  await audio.play();
}

async function playLoop(item) {
  stopErrorMix(item);
  stopCue(item);

  if (item.loop_mode === "one_shot") {
    await playOnce(item);
    return;
  }

  if (item.loop_mode === "continuous_loop" && item.category === "error") {
    await playErrorMixLoop(item);
    return;
  }

  const key = cueItemKey(item);
  const audio = new Audio(resolveAssetUrl(item.wav_path));
  audio.volume = getGlobalVolume();

  const state = {
    audio,
    timerId: 0,
    stopped: false,
    mode: "loop",
  };

  activePlayers.set(key, state);
  setRowPlaying(item, true);

  if (item.loop_mode === "continuous_loop") {
    audio.loop = true;
    await audio.play();
    return;
  }

  const runIntervalLoop = async () => {
    const latest = activePlayers.get(key);
    if (!latest || latest.stopped) {
      return;
    }

    const startAt = performance.now();
    latest.audio.currentTime = 0;
    latest.audio.volume = getGlobalVolume();

    latest.audio.onended = () => {
      const interval = getWarningLoopInterval(item.loop_interval_ms || 2000);
      const active = activePlayers.get(key);
      if (!active || active.stopped) {
        return;
      }
      const elapsed = performance.now() - startAt;
      const nextDelay = Math.max(0, interval - elapsed);
      active.timerId = window.setTimeout(() => {
        runIntervalLoop().catch(() => {
          stopCue(item);
        });
      }, nextDelay);
    };

    try {
      await latest.audio.play();
    } catch {
      stopCue(item);
      return;
    }
  };

  await runIntervalLoop();
}

function bindRowActions(item, row) {
  row.querySelector(".play-once").addEventListener("click", async () => {
    try {
      await playOnce(item);
    } catch {
      stopCue(item);
    }
  });

  const loopButton = row.querySelector(".play-loop");
  if (item.loop_mode === "one_shot") {
    loopButton.disabled = true;
    loopButton.textContent = "仅单次";
    loopButton.title = "该提示音仅支持单次播放";
  } else {
    loopButton.addEventListener("click", async () => {
      try {
        await playLoop(item);
      } catch {
        stopCue(item);
      }
    });
  }

  row.querySelector(".stop-one").addEventListener("click", () => {
    stopCue(item);
    stopErrorMix(item);
  });
}

function renderCue(item) {
  const fragment = template.content.cloneNode(true);
  const row = fragment.querySelector(".cue-item");
  row.dataset.cueKey = cueItemKey(item);

  const title = row.querySelector(".cue-title");
  const cueId = row.querySelector(".cue-id");
  const trigger = row.querySelector(".cue-trigger");
  const meta = row.querySelector(".cue-meta");
  const wav = row.querySelector(".open-wav");
  const mid = row.querySelector(".open-mid");

  title.textContent = item.title_zh;
  cueId.textContent = item.id;
  trigger.textContent = item.trigger_condition_zh;

  const durationText = `${(item.duration_ms / 1000).toFixed(2)}s`;
  const loopText =
    item.loop_mode === "one_shot"
      ? "单次"
      : item.loop_mode === "interval_loop"
        ? `间隔循环(${item.loop_interval_ms}ms)`
        : "连续循环";
  meta.textContent = `${groupTitles[item.category]} · ${loopText} · 时长 ${durationText}`;

  wav.href = resolveAssetUrl(item.wav_path);
  mid.href = resolveAssetUrl(item.mid_path);

  bindRowActions(item, row);
  return fragment;
}

function render(manifestData) {
  for (const key of Object.keys(sections)) {
    sections[key].innerHTML = "";
  }

  for (const item of manifestData.items) {
    const target = sections[item.category];
    if (!target) continue;
    target.appendChild(renderCue(item));
  }

  warningIntervalEl.value = String(manifestData.warning_interval_ms_default);
}

function applyFilter() {
  const selected = filterEl.value;
  for (const [category] of Object.entries(sections)) {
    const groupSection = document.getElementById(`${category}-group`);
    if (!groupSection) continue;
    if (selected === "all" || selected === category) {
      groupSection.style.display = "block";
    } else {
      groupSection.style.display = "none";
    }
  }
}

function bindGlobalEvents() {
  filterEl.addEventListener("change", applyFilter);
  volumeEl.addEventListener("input", () => {
    const volume = getGlobalVolume();
    volumeValueEl.textContent = volume.toFixed(2);

    for (const state of activePlayers.values()) {
      if (state.audio) {
        state.audio.volume = volume;
      }
    }
    syncErrorMixerGains();
  });
  stopAllEl.addEventListener("click", stopAllCues);
}

async function bootstrap() {
  bindGlobalEvents();
  applyFilter();

  const response = await fetch(MANIFEST_URL);
  if (!response.ok) {
    throw new Error(`Failed to load manifest: ${response.status}`);
  }
  manifestBaseUrl = response.url;
  manifest = await response.json();
  if (manifest.tempo_bpm && manifest.bar_beats) {
    const tempo = Number(manifest.tempo_bpm);
    const barBeats = Number(manifest.bar_beats);
    if (Number.isFinite(tempo) && tempo > 0 && Number.isFinite(barBeats) && barBeats > 0) {
      errorBeatSeconds = 60 / tempo;
      errorBarSeconds = errorBeatSeconds * barBeats;
    }
  }
  render(manifest);
}

bootstrap().catch((error) => {
  console.error(error);
  const container = document.querySelector(".layout");
  const message = document.createElement("section");
  message.className = "panel";
  message.innerHTML = `<h2>加载失败</h2><p>无法读取 cues.manifest.json，请确认已生成音频资产。</p>`;
  container.appendChild(message);
});
