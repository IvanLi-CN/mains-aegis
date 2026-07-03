import { useEffect, useRef, useState } from "react";

type AudioCueRow = {
  id: string;
  title?: string;
  purpose?: string;
  route?: string;
  category?: string;
  semantics: string;
  src: string;
};

function resolveAssetPath(src: string): string {
  if (/^(https?:|data:|blob:)/.test(src)) {
    return src;
  }

  const env = import.meta.env as { BASE_URL?: string } | undefined;
  const base = env?.BASE_URL ?? "/";
  return `${base.replace(/\/$/, "")}/${src.replace(/^\//, "")}`;
}

function AudioCueButton({ src, label }: { src: string; label: string }) {
  const audioRef = useRef<HTMLAudioElement | null>(null);
  const [isPlaying, setIsPlaying] = useState(false);

  useEffect(() => {
    return () => {
      audioRef.current?.pause();
      audioRef.current = null;
    };
  }, []);

  const stopPlayback = () => {
    audioRef.current?.pause();
    if (audioRef.current) {
      audioRef.current.currentTime = 0;
    }
    audioRef.current = null;
    setIsPlaying(false);
  };

  const playCue = () => {
    if (isPlaying) {
      stopPlayback();
      return;
    }

    audioRef.current?.pause();
    const audio = new Audio(resolveAssetPath(src));
    audioRef.current = audio;
    setIsPlaying(true);

    const clear = () => {
      if (audioRef.current === audio) {
        audioRef.current = null;
      }
      setIsPlaying(false);
    };

    audio.addEventListener("ended", clear, { once: true });
    audio.addEventListener("error", clear, { once: true });
    void audio.play().catch(clear);
  };

  return (
    <button
      type="button"
      className="audio-cue-play-button"
      onClick={playCue}
      aria-pressed={isPlaying}
      aria-label={`${isPlaying ? "停止" : "播放"} ${label}`}
      title={`${isPlaying ? "停止" : "播放"} ${label}`}
    >
      {isPlaying ? (
        <svg aria-hidden="true" viewBox="0 0 24 24" className="audio-cue-play-icon">
          <rect x="7" y="7" width="10" height="10" rx="1.5" />
        </svg>
      ) : (
        <svg aria-hidden="true" viewBox="0 0 24 24" className="audio-cue-play-icon">
          <path d="M9 7.8v8.4c0 .64.7 1.02 1.23.66l6.2-4.2a.78.78 0 0 0 0-1.32l-6.2-4.2A.78.78 0 0 0 9 7.8Z" />
        </svg>
      )}
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
                <AudioCueButton src={row.src} label={row.id} />
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
                <AudioCueButton src={row.src} label={row.id} />
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}
