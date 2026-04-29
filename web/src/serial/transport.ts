import type { Identity, UpsStatus } from "../api/types";

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

type SerialPortLike = EventTarget & {
  readable: ReadableStream<Uint8Array> | null;
  writable: WritableStream<Uint8Array> | null;
  open: (options: SerialPortOpenOptions) => Promise<void>;
  close: () => Promise<void>;
};

type SerialLike = EventTarget & {
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
    safe_settings: boolean;
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

type SerialTransportOptions = {
  onFrame: (frame: SerialFrame) => void;
  onClose: (error?: Error) => void;
};

const BAUD_RATE = 115200;
const RESPONSE_TIMEOUT_MS = 5000;

export function isWebSerialSupported(): boolean {
  return typeof navigator !== "undefined" && Boolean(navigator.serial);
}

export class WebSerialTransport {
  private reader: ReadableStreamDefaultReader<Uint8Array> | null = null;
  private writer: WritableStreamDefaultWriter<Uint8Array> | null = null;
  private readBuffer = "";
  private readonly decoder = new TextDecoder();
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
    const port = await navigator.serial.requestPort();
    await port.open({ baudRate: BAUD_RATE });
    const transport = new WebSerialTransport(port, options);
    transport.startReadLoop();
    return transport;
  }

  async close(): Promise<void> {
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

  async hello(): Promise<SerialHelloFrame> {
    const requestId = nextRequestId();
    const frame = await this.sendAndWait<SerialHelloFrame>({ type: "hello", request_id: requestId }, requestId);
    if (frame.type !== "hello") throw new Error("USB CDC handshake returned an unexpected frame");
    return frame;
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
  ): Promise<TFrame> {
    const waiter = new Promise<SerialResponseFrame | SerialHelloFrame>((resolve, reject) => {
      const timer = window.setTimeout(() => {
        this.pending.delete(requestId);
        reject(new Error("USB CDC response timed out"));
      }, RESPONSE_TIMEOUT_MS);
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
    await this.writer.write(this.encoder.encode(`${JSON.stringify(frame)}\n`));
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
        this.consumeText(this.decoder.decode(value, { stream: true }));
      }
      this.options.onClose();
    } catch (error) {
      this.options.onClose(error instanceof Error ? error : new Error("serial read failed"));
    }
  }

  private consumeText(text: string) {
    this.readBuffer += text;
    for (;;) {
      const newlineIndex = this.readBuffer.indexOf("\n");
      if (newlineIndex === -1) return;
      const rawLine = this.readBuffer.slice(0, newlineIndex).trim();
      this.readBuffer = this.readBuffer.slice(newlineIndex + 1);
      if (!rawLine) continue;
      const candidate = this.extractJsonCandidate(rawLine);
      if (!candidate) continue;
      let frame: SerialFrame;
      try {
        frame = JSON.parse(candidate) as SerialFrame;
      } catch {
        this.options.onFrame({
          type: "error",
          error: {
            code: "serial_parse_error",
            message: "received an invalid JSON line from USB CDC",
            retryable: true,
            details: null,
          },
        });
        continue;
      }
      this.options.onFrame(frame);
      this.resolvePending(frame);
    }
  }

  private extractJsonCandidate(rawLine: string): string | null {
    const jsonStart = rawLine.indexOf("{");
    if (jsonStart === -1) {
      this.emitRawSerialLog(rawLine);
      return null;
    }
    if (jsonStart > 0) {
      this.emitRawSerialLog(rawLine.slice(0, jsonStart));
    }
    return rawLine.slice(jsonStart);
  }

  private emitRawSerialLog(rawLine: string) {
    const message = rawLine.replace(/[^\x20-\x7e]+/g, " ").trim();
    if (!message) return;
    this.options.onFrame({
      type: "log",
      level: "debug",
      target: "raw_serial",
      message: message.slice(0, 240),
    });
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
      message: "USB CDC port is already open or unavailable",
      retryable: true,
      details: null,
    };
  }
  if (error instanceof Error) {
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

function nextRequestId(): string {
  return `web-${Date.now().toString(36)}-${Math.random().toString(16).slice(2, 8)}`;
}
