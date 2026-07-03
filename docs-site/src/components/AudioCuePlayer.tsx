import { useEffect, useRef, useState } from "react";

type AudioCueRow = {
  id: string;
  title?: string;
  purpose?: string;
  route?: string;
  category?: string;
  semantics: string;
  src: string;
  repeatCount?: number;
};

function resolveAssetPath(src: string): string {
  if (/^(https?:|data:|blob:)/.test(src)) {
    return src;
  }

  const env = import.meta.env as { BASE_URL?: string } | undefined;
  const base = env?.BASE_URL ?? "/";
  return `${base.replace(/\/$/, "")}/${src.replace(/^\//, "")}`;
}

function AudioCueButton({
  src,
  label,
  repeatCount = 1,
}: {
  src: string;
  label: string;
  repeatCount?: number;
}) {
  const audioRef = useRef<HTMLAudioElement | null>(null);
  const runIdRef = useRef(0);
  const repeatDelayRef = useRef<number | null>(null);
  const [isPlaying, setIsPlaying] = useState(false);
  const [playIndex, setPlayIndex] = useState(0);

  useEffect(() => {
    return () => {
      runIdRef.current += 1;
      if (repeatDelayRef.current !== null) {
        window.clearTimeout(repeatDelayRef.current);
      }
      audioRef.current?.pause();
      audioRef.current = null;
    };
  }, []);

  const stopPlayback = () => {
    runIdRef.current += 1;
    if (repeatDelayRef.current !== null) {
      window.clearTimeout(repeatDelayRef.current);
      repeatDelayRef.current = null;
    }
    audioRef.current?.pause();
    if (audioRef.current) {
      audioRef.current.currentTime = 0;
    }
    audioRef.current = null;
    setIsPlaying(false);
    setPlayIndex(0);
  };

  const playCue = () => {
    if (isPlaying) {
      stopPlayback();
      return;
    }

    const totalRepeats = Math.max(1, Math.floor(repeatCount));
    const runId = runIdRef.current + 1;
    runIdRef.current = runId;
    setIsPlaying(true);
    setPlayIndex(1);

    const clear = (audio?: HTMLAudioElement) => {
      if (!audio || audioRef.current === audio) {
        audioRef.current = null;
      }
      setIsPlaying(false);
      setPlayIndex(0);
    };

    const playIteration = (iteration: number) => {
      if (runIdRef.current !== runId) {
        return;
      }

      setPlayIndex(iteration);
      const audio = new Audio(resolveAssetPath(src));
      audioRef.current = audio;
      const startedAt = performance.now();

      audio.addEventListener(
        "ended",
        () => {
          if (runIdRef.current !== runId) {
            return;
          }

          const finishIteration = () => {
            repeatDelayRef.current = null;
            if (runIdRef.current !== runId) {
              return;
            }
            if (iteration < totalRepeats) {
              playIteration(iteration + 1);
            } else {
              clear(audio);
            }
          };
          const visibleDelayMs =
            totalRepeats > 1 ? Math.max(0, 650 - (performance.now() - startedAt)) : 0;
          repeatDelayRef.current = window.setTimeout(finishIteration, visibleDelayMs);
        },
        { once: true },
      );
      audio.addEventListener("error", () => clear(audio), { once: true });
      void audio.play().catch(() => clear(audio));
    };

    playIteration(1);
  };

  const visibleLabel =
    isPlaying && repeatCount > 1 ? (
      <span className="audio-cue-play-count" aria-hidden="true">
        {playIndex}
      </span>
    ) : isPlaying ? (
      <svg aria-hidden="true" viewBox="0 0 24 24" className="audio-cue-play-icon">
        <rect x="7" y="7" width="10" height="10" rx="1.5" />
      </svg>
    ) : (
      <svg aria-hidden="true" viewBox="0 0 24 24" className="audio-cue-play-icon">
        <path d="M9 7.8v8.4c0 .64.7 1.02 1.23.66l6.2-4.2a.78.78 0 0 0 0-1.32l-6.2-4.2A.78.78 0 0 0 9 7.8Z" />
      </svg>
    );
  const actionLabel = isPlaying
    ? repeatCount > 1
      ? `停止 ${label}，正在播放第 ${playIndex}/${repeatCount} 次`
      : `停止 ${label}`
    : repeatCount > 1
      ? `播放 ${label}，连播 ${repeatCount} 次`
      : `播放 ${label}`;

  return (
    <button
      type="button"
      className="audio-cue-play-button"
      onClick={playCue}
      aria-pressed={isPlaying}
      aria-label={actionLabel}
      title={actionLabel}
    >
      {visibleLabel}
    </button>
  );
}

export function InteractionAudioCueTable({ rows }: { rows: AudioCueRow[] }) {
  return (
    <div className="audio-cue-table-wrap">
      <table className="audio-cue-table audio-cue-table--interaction">
        <thead>
          <tr>
            <th>ID</th>
            <th>用途</th>
            <th>路由</th>
            <th>触发语义</th>
            <th>预览</th>
          </tr>
        </thead>
        <tbody>
          {rows.map((row) => (
            <tr key={row.id}>
              <td data-label="ID">
                <code>{row.id}</code>
              </td>
              <td data-label="用途">{row.purpose}</td>
              <td data-label="路由">
                <code>{row.route}</code>
              </td>
              <td data-label="触发语义">{row.semantics}</td>
              <td data-label="预览">
                <AudioCueButton src={row.src} label={row.id} repeatCount={row.repeatCount} />
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

export function SystemAudioCueTable({ rows }: { rows: AudioCueRow[] }) {
  return (
    <div className="audio-cue-table-wrap">
      <table className="audio-cue-table audio-cue-table--system">
        <thead>
          <tr>
            <th>ID</th>
            <th>标题</th>
            <th>分类</th>
            <th>触发语义</th>
            <th>预览</th>
          </tr>
        </thead>
        <tbody>
          {rows.map((row) => (
            <tr key={row.id}>
              <td data-label="ID">
                <code>{row.id}</code>
              </td>
              <td data-label="标题">{row.title}</td>
              <td data-label="分类">{row.category}</td>
              <td data-label="触发语义">{row.semantics}</td>
              <td data-label="预览">
                <AudioCueButton
                  src={row.src}
                  label={row.id}
                  repeatCount={row.repeatCount ?? (row.category === "warning" || row.category === "error" ? 5 : 1)}
                />
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}
