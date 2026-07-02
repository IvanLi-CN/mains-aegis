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
      aria-label={`${isPlaying ? "停止" : "播放"} ${label}`}
    >
      {isPlaying ? "停止" : "播放"}
    </button>
  );
}

export function InteractionAudioCueTable({ rows }: { rows: AudioCueRow[] }) {
  return (
    <div className="audio-cue-table-wrap">
      <table className="audio-cue-table">
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
              <td>
                <code>{row.id}</code>
              </td>
              <td>{row.purpose}</td>
              <td>
                <code>{row.route}</code>
              </td>
              <td>{row.semantics}</td>
              <td>
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
      <table className="audio-cue-table">
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
              <td>
                <code>{row.id}</code>
              </td>
              <td>{row.title}</td>
              <td>{row.category}</td>
              <td>{row.semantics}</td>
              <td>
                <AudioCueButton src={row.src} label={row.id} />
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}
