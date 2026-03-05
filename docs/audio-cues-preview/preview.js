const MANIFEST_URL = "./cues.manifest.json";

const groupTitles = {
  status: "状态音",
  warning: "告警音",
  error: "错误音",
};

const activePlayers = new Map();
let manifest = null;
let manifestBaseUrl = new URL(MANIFEST_URL, window.location.href).toString();

const filterEl = document.getElementById("filter");
const volumeEl = document.getElementById("volume");
const volumeValueEl = document.getElementById("volume-value");
const warningIntervalEl = document.getElementById("warning-interval");
const stopAllEl = document.getElementById("stop-all");
const errorMixListEl = document.getElementById("error-mix-list");
const errorMixPlayEl = document.getElementById("error-mix-play");
const errorMixStopEl = document.getElementById("error-mix-stop");
const errorMixStatusEl = document.getElementById("error-mix-status");

const sections = {
  status: document.querySelector('#status-group .cue-list'),
  warning: document.querySelector('#warning-group .cue-list'),
  error: document.querySelector('#error-group .cue-list'),
};

const template = document.getElementById("cue-template");

const ERROR_COLOR_GROUP = {
  shutdown_protection: "yellow",
  io_over_voltage: "green",
  io_over_current: "green",
  io_over_power: "green",
  module_fault: "blue",
  battery_protection: "red",
};

const activeErrorMixers = new Map();
const mixBufferCache = new Map();
let mixAudioContext = null;

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

function getErrorItems() {
  if (!manifest) return [];
  return manifest.items.filter((item) => item.category === "error");
}

function findCueById(cueId) {
  if (!manifest) return null;
  return manifest.items.find((item) => item.id === cueId) ?? null;
}

function refreshRowPlaying(item) {
  const key = cueItemKey(item);
  setRowPlaying(item, activePlayers.has(key) || activeErrorMixers.has(key));
}

function getErrorMixScale(count) {
  if (count <= 1) return 1.0;
  if (count === 2) return 0.66;
  return 0.52;
}

function syncErrorMixVolumes() {
  const volume = getGlobalVolume() * getErrorMixScale(activeErrorMixers.size);
  for (const state of activeErrorMixers.values()) {
    state.gain.gain.value = volume;
  }
}

function stopErrorMixItem(item) {
  const key = cueItemKey(item);
  const state = activeErrorMixers.get(key);
  if (!state) return;
  try {
    state.source.stop();
  } catch {
    // Source might already be stopped.
  }
  state.source.disconnect();
  state.gain.disconnect();
  activeErrorMixers.delete(key);
  syncErrorMixVolumes();
  refreshRowPlaying(item);
}

function stopAllErrorMixes() {
  if (!manifest) return;
  for (const item of getErrorItems()) {
    stopErrorMixItem(item);
  }
}

function setErrorMixStatus(text, kind = "") {
  if (!errorMixStatusEl) return;
  errorMixStatusEl.textContent = text;
  errorMixStatusEl.classList.remove("ok", "warn");
  if (kind === "ok" || kind === "warn") {
    errorMixStatusEl.classList.add(kind);
  }
}

async function ensureMixContext() {
  if (mixAudioContext) return mixAudioContext;
  const Ctx = window.AudioContext || window.webkitAudioContext;
  if (!Ctx) {
    throw new Error("当前浏览器不支持 Web Audio API");
  }
  mixAudioContext = new Ctx();
  return mixAudioContext;
}

async function loadMixBuffer(item) {
  const key = cueItemKey(item);
  if (mixBufferCache.has(key)) {
    return mixBufferCache.get(key);
  }
  const ctx = await ensureMixContext();
  const response = await fetch(resolveAssetUrl(item.wav_path));
  if (!response.ok) {
    throw new Error(`加载失败: ${item.id}`);
  }
  const arrayBuffer = await response.arrayBuffer();
  const audioBuffer = await ctx.decodeAudioData(arrayBuffer);
  mixBufferCache.set(key, audioBuffer);
  return audioBuffer;
}

function selectedErrorMixItems() {
  if (!errorMixListEl) return [];
  const selected = errorMixListEl.querySelectorAll("input[type='checkbox']:checked");
  return Array.from(selected)
    .map((input) => findCueById(input.value))
    .filter(Boolean);
}

function validateErrorMixSelection(items) {
  if (items.length < 2 || items.length > 3) {
    return "请勾选 2~3 个错误项进行组合试听";
  }
  const colorGroups = new Set();
  for (const item of items) {
    const colorGroup = ERROR_COLOR_GROUP[item.id] ?? item.id;
    if (colorGroups.has(colorGroup)) {
      return "同颜色错误项理论上不会并发，请改为不同颜色组合";
    }
    colorGroups.add(colorGroup);
  }
  return "";
}

async function playSelectedErrorMix() {
  const items = selectedErrorMixItems();
  const invalid = validateErrorMixSelection(items);
  if (invalid) {
    setErrorMixStatus(invalid, "warn");
    return;
  }

  stopAllErrorMixes();
  for (const item of items) {
    stopCue(item);
  }

  const ctx = await ensureMixContext();
  await ctx.resume();
  const startAt = ctx.currentTime + 0.1;

  for (const item of items) {
    const key = cueItemKey(item);
    const buffer = await loadMixBuffer(item);
    const source = ctx.createBufferSource();
    const gain = ctx.createGain();
    source.buffer = buffer;
    source.loop = true;
    source.connect(gain);
    gain.connect(ctx.destination);
    source.onended = () => {
      const latest = activeErrorMixers.get(key);
      if (!latest || latest.source !== source) return;
      activeErrorMixers.delete(key);
      syncErrorMixVolumes();
      refreshRowPlaying(item);
    };
    activeErrorMixers.set(key, { item, source, gain });
    source.start(startAt);
    refreshRowPlaying(item);
  }

  syncErrorMixVolumes();
  setErrorMixStatus(`组合播放中：${items.map((item) => item.id).join(" + ")}`, "ok");
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
  setErrorMixStatus("");
}

async function playOnce(item) {
  stopErrorMixItem(item);
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
  stopErrorMixItem(item);
  stopCue(item);

  if (item.loop_mode === "one_shot") {
    await playOnce(item);
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
    stopErrorMixItem(item);
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

function renderErrorMixOptions(manifestData) {
  if (!errorMixListEl) return;
  errorMixListEl.innerHTML = "";
  const errorItems = manifestData.items.filter((item) => item.category === "error");
  for (const item of errorItems) {
    const label = document.createElement("label");
    const input = document.createElement("input");
    input.type = "checkbox";
    input.value = item.id;
    label.appendChild(input);
    label.append(` ${item.title_zh}`);
    errorMixListEl.appendChild(label);
  }
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
    syncErrorMixVolumes();
  });
  stopAllEl.addEventListener("click", stopAllCues);

  if (errorMixPlayEl) {
    errorMixPlayEl.addEventListener("click", () => {
      playSelectedErrorMix().catch((error) => {
        console.error(error);
        setErrorMixStatus("组合播放启动失败，请重试", "warn");
      });
    });
  }
  if (errorMixStopEl) {
    errorMixStopEl.addEventListener("click", () => {
      stopAllErrorMixes();
      setErrorMixStatus("");
    });
  }
  if (errorMixListEl) {
    errorMixListEl.addEventListener("change", () => {
      const selected = selectedErrorMixItems();
      if (selected.length === 0) {
        setErrorMixStatus("");
        return;
      }
      const invalid = validateErrorMixSelection(selected);
      if (invalid) {
        setErrorMixStatus(invalid, "warn");
        return;
      }
      setErrorMixStatus(`已选组合：${selected.map((item) => item.id).join(" + ")}`);
    });
  }
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
  render(manifest);
  renderErrorMixOptions(manifest);
}

bootstrap().catch((error) => {
  console.error(error);
  const container = document.querySelector(".layout");
  const message = document.createElement("section");
  message.className = "panel";
  message.innerHTML = `<h2>加载失败</h2><p>无法读取 cues.manifest.json，请确认已生成音频资产。</p>`;
  container.appendChild(message);
});
