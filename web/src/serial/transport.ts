import type { DefmtDecodeResult, Identity, SerialTraceEntry, UpsStatus } from "../api/types";

type SerialPortRequestOptions = {
  filters?: Array<{ usbVendorId?: number; usbProductId?: number }>;
};

type SerialPortOpenOptions = {
  baudRate: number;
  dataBits?: 7 | 8;
  stopBits?: 1 | 2;
  parity?: "none" | "even" | "odd";
  bufferSize?: number;
  flowControl?: "none" | "hardware";
};

export type SerialPortLike = EventTarget & {
  readable: ReadableStream<Uint8Array> | null;
  writable: WritableStream<Uint8Array> | null;
  open: (options: SerialPortOpenOptions) => Promise<void>;
  setSignals?: (signals: { dataTerminalReady?: boolean; requestToSend?: boolean }) => Promise<void>;
  close: () => Promise<void>;
};

export type SerialLike = EventTarget & {
  requestPort: (options?: SerialPortRequestOptions) => Promise<SerialPortLike>;
};

declare global {
  interface Navigator {
    serial?: SerialLike;
  }
}

export type SerialHelloFrame = {
  type: "hello";
  request_id?: string;
  protocol: string;
  framing: "jsonl" | string;
  capabilities: {
    status: boolean;
    structured_logs: boolean;
    settings: boolean;
    wifi_config: boolean;
    psk_echo: false;
  };
  identity: Identity;
};

export type SerialStatusFrame = {
  type: "status";
  status: UpsStatus;
};

export type SerialLogFrame = {
  type: "log";
  level: string;
  target?: string;
  message: string;
};

export type SerialResponseFrame = {
  type: "response";
  request_id: string;
  ok: true;
  result: unknown;
};

export type SerialErrorFrame = {
  type: "error";
  request_id?: string;
  error: {
    code: string;
    message: string;
    retryable: boolean;
    details: unknown | null;
  };
};

export type SerialFrame = SerialHelloFrame | SerialStatusFrame | SerialLogFrame | SerialResponseFrame | SerialErrorFrame;
export type SerialTraceEvent = Omit<SerialTraceEntry, "id" | "timestamp">;

type SerialTransportOptions = {
  onFrame: (frame: SerialFrame) => void;
  onTrace: (entry: SerialTraceEvent) => void;
  onDefmtLog?: (entry: DefmtDecodeResult, frameHex: string) => void;
  onClose: (error?: Error) => void;
};

const BAUD_RATE = 115200;
const RESPONSE_TIMEOUT_MS = 5000;
const HELLO_TIMEOUT_MS = 8000;
const HELLO_ATTEMPT_TIMEOUT_MS = 1200;
const HELLO_RETRY_INTERVAL_MS = 250;
const MAX_LINE_BYTES = 16 * 1024;
export const ESPRESSIF_USB_SERIAL_FILTERS = [
  { usbVendorId: 0x303a },
  { usbVendorId: 0x10c4 },
  { usbVendorId: 0x1a86 },
];

export function isWebSerialSupported(): boolean {
  return typeof navigator !== "undefined" && Boolean(navigator.serial);
}

export class WebSerialTransport {
  private reader: ReadableStreamDefaultReader<Uint8Array> | null = null;
  private writer: WritableStreamDefaultWriter<Uint8Array> | null = null;
  private suppressCloseNotification = false;
  private readBuffer: number[] = [];
  private defmtBuffer: number[] = [];
  private defmtInFrame = false;
  private defmtDecoder: ((frame: Uint8Array) => Promise<DefmtDecodeResult>) | null = null;
  private readonly decoder = new TextDecoder("utf-8", { fatal: true });
  private readonly encoder = new TextEncoder();
  private readonly pending = new Map<
    string,
    {
      resolve: (frame: SerialResponseFrame | SerialHelloFrame) => void;
      reject: (error: SerialErrorFrame["error"]) => void;
      timer: number;
    }
  >();

  private constructor(
    private readonly port: SerialPortLike,
    private readonly options: SerialTransportOptions,
  ) {}

  static async request(options: SerialTransportOptions): Promise<WebSerialTransport> {
    if (!navigator.serial) {
      throw new Error("This browser does not expose the Web Serial API");
    }
    const port = await navigator.serial.requestPort({ filters: ESPRESSIF_USB_SERIAL_FILTERS });
    await port.open({ baudRate: BAUD_RATE });
    const transport = new WebSerialTransport(port, options);
    transport.startReadLoop();
    return transport;
  }

  async close(): Promise<void> {
    await this.closePort();
  }

  async releasePort(): Promise<SerialPortLike> {
    await this.closePort();
    return this.port;
  }

  private async closePort(): Promise<void> {
    this.suppressCloseNotification = true;
    for (const pending of this.pending.values()) {
      window.clearTimeout(pending.timer);
      pending.reject({
        code: "serial_closed",
        message: "serial connection closed",
        retryable: true,
        details: null,
      });
    }
    this.pending.clear();
    await this.reader?.cancel().catch(() => undefined);
    this.reader?.releaseLock();
    this.reader = null;
    this.writer?.releaseLock();
    this.writer = null;
    await this.port.close().catch(() => undefined);
  }

  setDefmtDecoder(decoder: ((frame: Uint8Array) => Promise<DefmtDecodeResult>) | null) {
    this.defmtDecoder = decoder;
  }

  async hello(): Promise<SerialHelloFrame> {
    const deadline = Date.now() + HELLO_TIMEOUT_MS;
    let lastError: unknown;
    while (Date.now() < deadline) {
      const requestId = nextRequestId();
      try {
        const frame = await this.sendAndWait<SerialHelloFrame>({ type: "hello", request_id: requestId }, requestId, HELLO_ATTEMPT_TIMEOUT_MS);
        if (frame.type !== "hello") throw new Error("USB CDC handshake returned an unexpected frame");
        return frame;
      } catch (error) {
        lastError = error;
        await sleep(HELLO_RETRY_INTERVAL_MS);
      }
    }
    const suffix = lastError instanceof Error ? ` Last error: ${lastError.message}` : "";
    throw new Error(`USB CDC did not return a Mains Aegis hello frame before timeout. The port opened, but the firmware did not answer.${suffix}`);
  }

  async requestIdentity(): Promise<Identity> {
    const result = await this.request("get_identity");
    return result as Identity;
  }

  async requestStatus(): Promise<UpsStatus> {
    const result = await this.request("get_status");
    return result as UpsStatus;
  }

  async setLogLevel(level: string): Promise<unknown> {
    return this.request("set_log_level", { level });
  }

  async setManualChargePrefs(payload: { target: string; speed: string; timer_h: number }): Promise<unknown> {
    return this.request("set_manual_charge_prefs", payload);
  }

  async setWifiConfig(ssid: string, psk: string): Promise<unknown> {
    const requestId = nextRequestId();
    const response = await this.sendAndWait<SerialResponseFrame>(
      { type: "wifi_config", request_id: requestId, op: "set", ssid, psk },
      requestId,
    );
    return response.result;
  }

  async clearWifiConfig(): Promise<unknown> {
    const requestId = nextRequestId();
    const response = await this.sendAndWait<SerialResponseFrame>(
      { type: "wifi_config", request_id: requestId, op: "clear" },
      requestId,
    );
    return response.result;
  }

  private async request(op: string, payload: Record<string, unknown> = {}): Promise<unknown> {
    const requestId = nextRequestId();
    const response = await this.sendAndWait<SerialResponseFrame>({ type: "request", request_id: requestId, op, ...payload }, requestId);
    return response.result;
  }

  private async sendAndWait<TFrame extends SerialResponseFrame | SerialHelloFrame>(
    frame: Record<string, unknown>,
    requestId: string,
    timeoutMs = RESPONSE_TIMEOUT_MS,
  ): Promise<TFrame> {
    const waiter = new Promise<SerialResponseFrame | SerialHelloFrame>((resolve, reject) => {
      const timer = window.setTimeout(() => {
        this.pending.delete(requestId);
        reject(new Error("USB CDC command timed out before a response frame was received."));
      }, timeoutMs);
      this.pending.set(requestId, {
        resolve,
        reject: (error) => reject(Object.assign(new Error(error.message), { envelope: error })),
        timer,
      });
    });
    await this.writeFrame(frame);
    return (await waiter) as TFrame;
  }

  private async writeFrame(frame: Record<string, unknown>): Promise<void> {
    if (!this.port.writable) throw new Error("Serial port is not writable");
    this.writer ??= this.port.writable.getWriter();
    const payload = JSON.stringify(frame);
    this.options.onTrace(traceFromFrame("tx", frame, JSON.stringify(redactTraceFrame(frame))));
    await this.writer.write(this.encoder.encode(`${payload}\n`));
  }

  private startReadLoop() {
    void this.readLoop();
  }

  private async readLoop() {
    try {
      if (!this.port.readable) throw new Error("Serial port is not readable");
      this.reader = this.port.readable.getReader();
      for (;;) {
        const { value, done } = await this.reader.read();
        if (done) break;
        if (!value) continue;
        this.consumeMonitorBytes(value);
      }
      if (!this.suppressCloseNotification) this.options.onClose();
    } catch (error) {
      if (this.suppressCloseNotification) return;
      this.options.onClose(error instanceof Error ? error : new Error("serial read failed"));
    }
  }

  private consumeMonitorBytes(bytes: Uint8Array) {
    this.defmtBuffer.push(...bytes);
    this.drainDefmtBuffer();
  }

  private drainDefmtBuffer() {
    for (;;) {
      if (this.defmtInFrame) {
        const end = findFrameEnd(this.defmtBuffer);
        if (end === -1) return;
        const frame = this.defmtBuffer.slice(0, end);
        this.defmtBuffer.splice(0, end + 1);
        this.defmtInFrame = false;
        this.consumeDefmtFrame(frame);
        continue;
      }

      const start = findFrameStart(this.defmtBuffer);
      if (start === -1) {
        const keep = this.defmtBuffer.at(-1) === 0xff ? 1 : 0;
        const raw = this.defmtBuffer.splice(0, this.defmtBuffer.length - keep);
        if (raw.length > 0) this.consumeLineBytesStream(raw);
        return;
      }

      const raw = this.defmtBuffer.splice(0, start);
      if (raw.length > 0) this.consumeLineBytesStream(raw);
      this.defmtBuffer.splice(0, 2);
      this.defmtInFrame = true;
    }
  }

  private consumeLineBytesStream(bytes: ArrayLike<number>) {
    for (let index = 0; index < bytes.length; index += 1) {
      const byte = bytes[index];
      if (byte === 10) {
        this.consumeLineBytes(this.readBuffer);
        this.readBuffer = [];
        continue;
      }
      this.readBuffer.push(byte);
      if (this.readBuffer.length > MAX_LINE_BYTES) {
        this.options.onTrace({
          direction: "rx",
          kind: "ignored",
          frameType: null,
          requestId: null,
          target: null,
          summary: "CDC line exceeded 16 KiB",
          payload: hexPreview(this.readBuffer),
        });
        this.readBuffer = [];
      }
    }
  }

  private consumeDefmtFrame(frame: number[]) {
    const frameHex = hexPayload(frame);
    if (!this.defmtDecoder) {
      this.options.onTrace({
        direction: "rx",
        kind: "defmt",
        frameType: "defmt",
        requestId: null,
        target: "defmt",
        summary: "defmt frame awaiting decoder",
        payload: frameHex,
      });
      return;
    }

    void this.defmtDecoder(new Uint8Array(frame))
      .then((decoded) => {
        this.options.onTrace({
          direction: "rx",
          kind: "raw",
          frameType: "defmt",
          requestId: null,
          target: decoded.target,
          summary: decoded.message,
          payload: decoded.message,
        });
        this.options.onDefmtLog?.(decoded, frameHex);
      })
      .catch((error) => {
        const failure = describeDefmtDecodeFailure(error);
        this.options.onTrace({
          direction: "rx",
          kind: "ignored",
          frameType: "defmt",
          requestId: null,
          target: "defmt_decode",
          summary: failure.summary,
          payload: `${failure.detail}\nframe_hex=${frameHex}`,
        });
      });
  }

  private consumeLineBytes(lineBytes: number[]) {
    if (lineBytes.length === 0) return;
    const jsonCandidate = this.extractJsonCandidate(lineBytes);
    if (jsonCandidate) {
      const { frame, payload } = jsonCandidate;
      this.options.onTrace(traceFromFrame("rx", frame, payload));
      this.options.onFrame(frame);
      this.resolvePending(frame);
      return;
    }

    const rawLine = this.decodeUtf8Line(lineBytes);
    if (rawLine === null) {
      this.options.onTrace({
        direction: "rx",
        kind: "defmt",
        frameType: "defmt",
        requestId: null,
        target: "defmt",
        summary: "defmt binary frame",
        payload: hexPreview(lineBytes),
      });
      return;
    }
    if (!rawLine) return;
    this.options.onTrace({
      direction: "rx",
      kind: "raw",
      frameType: null,
      requestId: null,
      target: null,
      summary: "raw CDC line",
      payload: rawLine,
    });
  }

  private extractJsonCandidate(lineBytes: number[]): { frame: SerialFrame; payload: string } | null {
    const bytes = new Uint8Array(lineBytes);
    for (let start = 0; start < bytes.length; start += 1) {
      if (bytes[start] !== 0x7b) continue;
      for (let end = bytes.length; end > start; end -= 1) {
        if (bytes[end - 1] !== 0x7d) continue;
        const payload = this.decodeUtf8Line(bytes.slice(start, end));
        if (payload === null) continue;
        try {
          return { frame: JSON.parse(payload) as SerialFrame, payload };
        } catch {
          continue;
        }
      }
    }
    return null;
  }

  private decodeUtf8Line(lineBytes: ArrayLike<number>): string | null {
    try {
      return this.decoder.decode(new Uint8Array(Array.from(lineBytes))).trim();
    } catch {
      return null;
    }
  }

  private resolvePending(frame: SerialFrame) {
    const requestId = "request_id" in frame ? frame.request_id : undefined;
    if (!requestId) return;
    const pending = this.pending.get(requestId);
    if (!pending) return;
    window.clearTimeout(pending.timer);
    this.pending.delete(requestId);
    if (frame.type === "error") {
      pending.reject(frame.error);
      return;
    }
    if (frame.type === "response" || frame.type === "hello") pending.resolve(frame);
  }
}

function describeDefmtDecodeFailure(error: unknown): { summary: string; detail: string } {
  const envelope = (error as { envelope?: { code?: string; message?: string; details?: unknown } } | null)?.envelope;
  if (envelope?.code) {
    return {
      summary: `${envelope.code}: ${envelope.message ?? "defmt decode failed"}`,
      detail: JSON.stringify({ source: "defmt_decode_api", ...envelope }),
    };
  }
  if (error instanceof Error) {
    return {
      summary: `defmt_decode_error: ${error.message}`,
      detail: JSON.stringify({ source: "defmt_decode_api", message: error.message }),
    };
  }
  return {
    summary: "defmt_decode_error: defmt decode failed",
    detail: JSON.stringify({ source: "defmt_decode_api", message: "defmt decode failed" }),
  };
}

export function errorFromSerialFailure(error: unknown): SerialErrorFrame["error"] {
  const maybeEnvelope = error as { envelope?: SerialErrorFrame["error"] };
  if (maybeEnvelope?.envelope) return maybeEnvelope.envelope;
  if (error instanceof DOMException && error.name === "NotFoundError") {
    return {
      code: "serial_permission_denied",
      message: "USB device selection was cancelled",
      retryable: true,
      details: null,
    };
  }
  if (error instanceof DOMException && error.name === "NetworkError") {
    return {
      code: "serial_port_unavailable",
      message: "USB CDC port is already open by devd or another app. Stop devd monitor/disconnect first, then retry Web Serial.",
      retryable: true,
      details: null,
    };
  }
  if (error instanceof Error) {
    const classified = classifySerialCommandError(error.message);
    if (classified) return classified;
    return {
      code: "serial_transport_error",
      message: error.message,
      retryable: true,
      details: null,
    };
  }
  return {
    code: "serial_unknown_error",
    message: "USB CDC connection failed",
    retryable: true,
    details: null,
  };
}

function classifySerialCommandError(message: string): SerialErrorFrame["error"] | null {
  const match = /^(wifi_(?:connect_failed|connect_timeout|disconnect_timeout)):\s*(.+)$/.exec(message);
  if (!match) return null;
  return {
    code: match[1],
    message: match[2],
    retryable: true,
    details: null,
  };
}

function nextRequestId(): string {
  return `web-${Date.now().toString(36)}-${Math.random().toString(16).slice(2, 8)}`;
}

function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => window.setTimeout(resolve, ms));
}

function hexPreview(bytes: ArrayLike<number>): string {
  const preview = hexPayload(Array.from(bytes).slice(0, 96));
  return bytes.length > 96 ? `${preview} ... (${bytes.length} bytes)` : preview;
}

function hexPayload(bytes: ArrayLike<number>): string {
  return Array.from(bytes)
    .map((byte) => byte.toString(16).padStart(2, "0"))
    .join(" ");
}

function findFrameStart(buffer: number[]): number {
  for (let index = 0; index < buffer.length - 1; index += 1) {
    if (buffer[index] === 0xff && buffer[index + 1] === 0x00) return index;
  }
  return -1;
}

function findFrameEnd(buffer: number[]): number {
  const start = buffer.findIndex((byte) => byte !== 0);
  if (start === -1) return -1;
  const end = buffer.slice(start).findIndex((byte) => byte === 0);
  return end === -1 ? -1 : start + end;
}

function traceFromFrame(direction: SerialTraceEntry["direction"], frame: Record<string, unknown>, payload: string): SerialTraceEvent {
  const frameType = typeof frame.type === "string" ? frame.type : null;
  const requestId = typeof frame.request_id === "string" ? frame.request_id : null;
  const target = typeof frame.target === "string" ? frame.target : null;
  return {
    direction,
    kind: "frame",
    frameType,
    requestId,
    target,
    summary: summarizeFrame(frame),
    payload,
  };
}

function summarizeFrame(frame: Record<string, unknown>): string {
  if (frame.type === "log") {
    return typeof frame.message === "string" ? frame.message : "log";
  }
  if (frame.type === "error") {
    const error = frame.error as { code?: unknown; message?: unknown } | undefined;
    return `${typeof error?.code === "string" ? error.code : "error"}: ${typeof error?.message === "string" ? error.message : "USB CDC error"}`;
  }
  if (frame.type === "response") {
    return "command response";
  }
  if (frame.type === "status") {
    return "status snapshot";
  }
  if (frame.type === "hello") {
    return "protocol handshake";
  }
  if (frame.type === "request") {
    return typeof frame.op === "string" ? frame.op : "request";
  }
  if (frame.type === "wifi_config") {
    return typeof frame.op === "string" ? `wifi_config ${frame.op}` : "wifi_config";
  }
  return typeof frame.type === "string" ? frame.type : "serial frame";
}

function redactTraceFrame(frame: Record<string, unknown>): Record<string, unknown> {
  if (frame.type === "wifi_config" && typeof frame.psk === "string") {
    return { ...frame, psk: "[redacted]" };
  }
  return frame;
}
