import { ESPLoader, Transport, type FlashOptions, type IEspLoaderTerminal } from "esptool-js";
import type { FirmwareArtifact, FirmwareArtifactMatch } from "../api/types";
import { firmwareArtifactFileUrl, firmwareArtifactImageFiles } from "./catalog";
import type { SerialLike, SerialPortLike } from "../serial/transport";

export type WebSerialFlashStage = "idle" | "request_port" | "connect" | "fetch" | "verify" | "write" | "reset" | "done" | "error";

export type WebSerialFlashProgress = {
  stage: WebSerialFlashStage;
  message: string;
  percent: number;
  written: number;
  total: number;
  fileIndex: number | null;
};

export type WebSerialFlashOptions = {
  artifact: FirmwareArtifact;
  artifactMatch: FirmwareArtifactMatch;
  onProgress: (progress: WebSerialFlashProgress) => void;
};

type FlashImage = {
  data: Uint8Array;
  address: number;
  size: number;
};

const ESPRESSIF_USB_FILTERS = [
  { usbVendorId: 0x303a },
  { usbVendorId: 0x10c4 },
  { usbVendorId: 0x1a86 },
];

export function isWebSerialFlashSupported(): boolean {
  return typeof navigator !== "undefined" && Boolean(navigator.serial);
}

export async function flashArtifactWithWebSerial({ artifact, artifactMatch, onProgress }: WebSerialFlashOptions): Promise<void> {
  const imageFiles = firmwareArtifactImageFiles(artifact);
  if (imageFiles.length === 0) {
    throw new Error("Selected artifact does not include Web Serial flash images with flash addresses");
  }
  if (!navigator.serial) {
    throw new Error("This browser does not expose the Web Serial API");
  }

  let transport: Transport | null = null;
  try {
    onProgress(makeProgress("request_port", "Select the ESP32-S3 serial port", 0, 0, 0, null));
    const port = await (navigator.serial as SerialLike).requestPort({ filters: ESPRESSIF_USB_FILTERS });
    transport = new Transport(port, true);
    const logs: string[] = [];
    const terminal: IEspLoaderTerminal = {
      clean: () => undefined,
      write: (line) => {
        if (line.trim()) logs.push(line.trim());
      },
      writeLine: (line) => {
        if (line.trim()) logs.push(line.trim());
      },
    };
    const loader = new ESPLoader({
      transport,
      baudrate: 921600,
      terminal,
      debugLogging: false,
    });

    onProgress(makeProgress("connect", "Connecting to ESP ROM loader", 0, 0, 0, null));
    const chip = await loader.main("default_reset");
    if (!chip.toLowerCase().includes("esp32-s3") && !chip.toLowerCase().includes("esp32s3")) {
      throw new Error(`Unexpected chip detected: ${chip}`);
    }

    onProgress(makeProgress("fetch", "Fetching firmware images", 5, 0, totalManifestBytes(imageFiles), null));
    const images = await Promise.all(
      imageFiles.map(async (file) => {
        const response = await fetch(firmwareArtifactFileUrl(artifactMatch, file.path));
        if (!response.ok) throw new Error(`Failed to fetch ${file.path}: HTTP ${response.status}`);
        const data = new Uint8Array(await response.arrayBuffer());
        return { file, data };
      }),
    );

    onProgress(makeProgress("verify", "Verifying firmware image hashes", 12, 0, totalManifestBytes(imageFiles), null));
    for (const image of images) {
      const actual = await sha256Hex(image.data);
      if (actual !== image.file.sha256) {
        throw new Error(`SHA-256 mismatch for ${image.file.path}`);
      }
    }

    const flashImages: FlashImage[] = images.map(({ file, data }) => ({
      data,
      address: file.flash_address,
      size: data.byteLength,
    }));
    const total = flashImages.reduce((sum, image) => sum + image.size, 0);
    const writtenByFile = new Map<number, number>();
    const flashOptions: FlashOptions = {
      fileArray: flashImages.map((image) => ({ data: image.data, address: image.address })),
      flashMode: "dio",
      flashFreq: "40m",
      flashSize: "detect",
      eraseAll: false,
      compress: true,
      reportProgress: (fileIndex, written, fileTotal) => {
        writtenByFile.set(fileIndex, Math.min(written, fileTotal));
        const aggregateWritten = Array.from(writtenByFile.values()).reduce((sum, value) => sum + value, 0);
        onProgress(makeProgress("write", "Writing firmware", 12 + Math.round((aggregateWritten / Math.max(total, 1)) * 78), aggregateWritten, total, fileIndex));
      },
    };

    onProgress(makeProgress("write", "Writing firmware", 12, 0, total, 0));
    await loader.writeFlash(flashOptions);
    onProgress(makeProgress("reset", "Resetting device", 96, total, total, null));
    await loader.after("hard_reset");
    onProgress(makeProgress("done", "Flash completed", 100, total, total, null));
  } catch (error) {
    onProgress(makeProgress("error", error instanceof Error ? error.message : "Web Serial flash failed", 0, 0, 0, null));
    throw error;
  } finally {
    await transport?.disconnect().catch(() => undefined);
  }
}

function makeProgress(
  stage: WebSerialFlashStage,
  message: string,
  percent: number,
  written: number,
  total: number,
  fileIndex: number | null,
): WebSerialFlashProgress {
  return {
    stage,
    message,
    percent: Math.max(0, Math.min(100, percent)),
    written,
    total,
    fileIndex,
  };
}

function totalManifestBytes(files: Array<{ size: number }>): number {
  return files.reduce((sum, file) => sum + file.size, 0);
}

async function sha256Hex(data: Uint8Array): Promise<string> {
  const copied = new Uint8Array(data.byteLength);
  copied.set(data);
  const digest = await crypto.subtle.digest("SHA-256", copied);
  return Array.from(new Uint8Array(digest))
    .map((byte) => byte.toString(16).padStart(2, "0"))
    .join("");
}
