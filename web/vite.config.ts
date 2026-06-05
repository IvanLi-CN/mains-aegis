import react from "@vitejs/plugin-react";
import { createHash } from "node:crypto";
import { createReadStream } from "node:fs";
import { mkdir, readdir, readFile, stat, writeFile } from "node:fs/promises";
import { basename, dirname, isAbsolute, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { defineConfig, type Plugin } from "vite";

const devdUrl = process.env.MAINS_AEGIS_DEVD_URL ?? process.env.VITE_DEFAULT_DEVD_URL ?? process.env.VITE_DEVD_API_BASE ?? "http://127.0.0.1:30080";
const appBase = normalizeBase(process.env.PAGES_BASE ?? process.env.VITE_BASE);
const webRoot = dirname(fileURLToPath(import.meta.url));
const repoRoot = resolve(webRoot, "..");
const publicFirmwareRoot = resolve(webRoot, "public/firmware");
const localFirmwareArtifactsRoot = resolve(repoRoot, process.env.MAINS_AEGIS_FIRMWARE_ARTIFACTS_DIR ?? "firmware/target/mains-aegis-artifacts");
const firmwareTargetRoot = resolve(repoRoot, "firmware/target");
const devFirmwareCacheRoot = resolve(repoRoot, "tmp/web-dev-firmware");

function normalizeBase(base: string | undefined): string {
  const raw = (base ?? "/").trim();
  if (!raw || raw === "/") return "/";
  const withLeading = raw.startsWith("/") ? raw : `/${raw}`;
  return withLeading.endsWith("/") ? withLeading : `${withLeading}/`;
}

type FirmwareArtifactFile = {
  kind: "elf" | "image" | "defmt_metadata";
  path: string;
  sha256: string;
  size: number;
  flash_address?: number | null;
};

type FirmwareArtifact = {
  artifact_id: string;
  name: string;
  version: string;
  git_sha: string;
  build_id: string;
  target_chip: "esp32s3";
  profile: "debug" | "release" | "dev";
  features: string[];
  protocol: "mains-aegis.cdc.v1";
  defmt: {
    enabled: boolean;
    encoding: string;
    elf_sha256: string | null;
    metadata_sha256: string | null;
  };
  files: FirmwareArtifactFile[];
  devd_manifest_path?: string;
};

type FirmwareArtifactRecord = {
  artifact: FirmwareArtifact;
  fileRoutes: Map<string, string>;
  priority: number;
};

type FirmwareIndex = {
  catalog: { schema_version: 1; artifacts: FirmwareArtifact[] };
  manifests: Map<string, FirmwareArtifact>;
  files: Map<string, string>;
};

function devFirmwarePlugin(): Plugin {
  let cache: { expiresAt: number; index: FirmwareIndex } | null = null;

  return {
    name: "mains-aegis-dev-firmware",
    apply: "serve",
    configureServer(server) {
      server.middlewares.use(async (req, res, next) => {
        const url = decodeURIComponent((req.url ?? "").split("?", 1)[0] ?? "");
        if (!url.startsWith("/firmware/")) {
          next();
          return;
        }

        try {
          const now = Date.now();
          if (!cache || cache.expiresAt <= now) {
            cache = { expiresAt: now + 750, index: await buildFirmwareIndex() };
          }
          const index = cache.index;
          if (url === "/firmware/firmware-catalog.json") {
            sendJson(res, index.catalog);
            return;
          }

          const firmwarePath = url.slice("/firmware/".length);
          const manifest = index.manifests.get(firmwarePath);
          if (manifest) {
            sendJson(res, manifest);
            return;
          }

          const source = index.files.get(firmwarePath);
          if (source) {
            res.statusCode = 200;
            res.setHeader("Content-Type", "application/octet-stream");
            createReadStream(source).pipe(res);
            return;
          }
        } catch (error) {
          server.config.logger.warn(`dev firmware catalog failed: ${error instanceof Error ? error.message : String(error)}`);
        }
        next();
      });
    },
  };
}

async function buildFirmwareIndex(): Promise<FirmwareIndex> {
  const records = [
    ...(await loadStaticFirmwareRecords()),
    ...(await loadLocalArtifactRecords()),
    ...(await loadDevdFirmwareRecords()),
  ];
  const byArtifact = new Map<string, FirmwareArtifactRecord>();
  for (const record of records) {
    const existing = byArtifact.get(record.artifact.artifact_id);
    if (!existing || existing.priority <= record.priority) {
      byArtifact.set(record.artifact.artifact_id, record);
    }
  }

  const artifacts = Array.from(byArtifact.values())
    .map((record) => record.artifact)
    .sort((left, right) => left.artifact_id.localeCompare(right.artifact_id));
  const manifests = new Map<string, FirmwareArtifact>();
  const files = new Map<string, string>();
  for (const record of byArtifact.values()) {
    manifests.set(`${record.artifact.artifact_id}.manifest.json`, record.artifact);
    for (const [route, source] of record.fileRoutes) files.set(route, source);
  }
  return { catalog: { schema_version: 1, artifacts }, manifests, files };
}

async function loadStaticFirmwareRecords(): Promise<FirmwareArtifactRecord[]> {
  return loadManifestRecords(publicFirmwareRoot, 10, false);
}

async function loadLocalArtifactRecords(): Promise<FirmwareArtifactRecord[]> {
  return loadManifestRecords(localFirmwareArtifactsRoot, 20, true);
}

async function loadManifestRecords(root: string, priority: number, strictFiles: boolean): Promise<FirmwareArtifactRecord[]> {
  const entries = await safeReadDir(root);
  const records: FirmwareArtifactRecord[] = [];
  for (const entry of entries) {
    if (!entry.endsWith(".manifest.json")) continue;
    const manifest = await readJson(join(root, entry));
    const record = await makeFirmwareRecord(manifest, root, priority, strictFiles);
    if (record) records.push(record);
  }

  const catalog = await readJson(join(root, "firmware-catalog.json"));
  if (catalog && Array.isArray(catalog.artifacts)) {
    for (const artifact of catalog.artifacts) {
      const record = await makeFirmwareRecord(artifact, root, priority, strictFiles);
      if (record) records.push(record);
    }
  }
  return records;
}

async function loadDevdFirmwareRecords(): Promise<FirmwareArtifactRecord[]> {
  const devicesPayload = await fetchJsonWithTimeout(`${devdUrl}/api/v1/devices`, 700);
  const devices = Array.isArray(devicesPayload?.devices) ? devicesPayload.devices : [];
  const records: FirmwareArtifactRecord[] = [];
  for (const device of devices) {
    const selected = await fetchJsonWithTimeout(`${devdUrl}/api/v1/devices/${encodeURIComponent(String(device.id))}/artifact`, 700);
    const selectedArtifact = selected?.artifact;
    if (selectedArtifact) {
      const record = await makeFirmwareRecord(selectedArtifact, null, 40, false);
      if (record) {
        records.push(record);
        continue;
      }
    }
    const identity = device?.identity;
    if (!isMainsIdentity(identity)) continue;
    records.push(makeObservedFirmwareRecord(identity));
  }
  return records;
}

async function makeFirmwareRecord(raw: unknown, sourceRoot: string | null, priority: number, strictFiles: boolean): Promise<FirmwareArtifactRecord | null> {
  if (!isFirmwareArtifact(raw)) return null;
  const artifactId = raw.artifact_id;
  const files: FirmwareArtifactFile[] = [];
  const devdFiles: FirmwareArtifactFile[] = [];
  const fileRoutes = new Map<string, string>();
  const rawFiles = Array.isArray(raw.files) ? raw.files : [];

  for (const file of rawFiles) {
    if (!isFirmwareFile(file)) {
      if (strictFiles) return null;
      continue;
    }
    const source = await resolveArtifactFile(sourceRoot, file);
    const stats = source ? await safeStat(source) : null;
    if (!source || !stats?.isFile()) {
      if (strictFiles) return null;
      continue;
    }
    const resolvedSource = source;
    if (stats.size !== file.size || (await sha256File(resolvedSource)) !== file.sha256) {
      if (strictFiles) return null;
      continue;
    }

    const browserPath = `${artifactId}/${basename(resolvedSource)}`;
    const browserFile: FirmwareArtifactFile = {
      kind: file.kind,
      path: browserPath,
      sha256: file.sha256,
      size: file.size,
    };
    if (typeof file.flash_address === "number") browserFile.flash_address = file.flash_address;
    files.push(browserFile);
    fileRoutes.set(browserPath, resolvedSource);

    const devdFile: FirmwareArtifactFile = { ...browserFile, path: resolvedSource };
    devdFiles.push(devdFile);
  }

  const artifact: FirmwareArtifact = {
    ...raw,
    defmt: {
      ...raw.defmt,
      elf_sha256: files.find((file) => file.kind === "elf")?.sha256 ?? raw.defmt.elf_sha256 ?? null,
      metadata_sha256: files.find((file) => file.kind === "defmt_metadata")?.sha256 ?? raw.defmt.metadata_sha256 ?? null,
    },
    files,
  };
  if (devdFiles.length > 0) {
    artifact.devd_manifest_path = await writeDevdManifest({ ...artifact, files: devdFiles });
  }
  return { artifact, fileRoutes, priority };
}

function makeObservedFirmwareRecord(identity: { firmware: Record<string, unknown> }): FirmwareArtifactRecord {
  const firmware = identity.firmware;
  const features = Array.isArray(firmware.features) ? firmware.features.filter((item): item is string => typeof item === "string") : [];
  const profile = isProfile(firmware.build_profile) ? firmware.build_profile : "dev";
  const buildId = typeof firmware.build_id === "string" ? firmware.build_id : "unknown";
  const gitSha = typeof firmware.git_sha === "string" ? firmware.git_sha : "unknown";
  const artifact: FirmwareArtifact = {
    artifact_id: `mains-aegis-esp32s3-${profile}-${features.join("-") || "default"}-${buildId}`,
    name: "mains-aegis",
    version: typeof firmware.package_version === "string" ? firmware.package_version : "0.1.0",
    git_sha: gitSha,
    git_dirty: typeof firmware.git_dirty === "string" ? firmware.git_dirty : "unknown",
    build_id: buildId,
    target_chip: "esp32s3",
    profile,
    features,
    protocol: "mains-aegis.cdc.v1",
    defmt: {
      enabled: true,
      encoding: "defmt-espflash",
      elf_sha256: null,
      metadata_sha256: null,
    },
    files: [],
  } as FirmwareArtifact;
  return { artifact, fileRoutes: new Map(), priority: 30 };
}

async function writeDevdManifest(artifact: FirmwareArtifact): Promise<string> {
  await mkdir(devFirmwareCacheRoot, { recursive: true });
  const path = join(devFirmwareCacheRoot, `${artifact.artifact_id}.manifest.json`);
  const { devd_manifest_path: _devdManifestPath, ...payload } = artifact;
  await writeFile(path, `${JSON.stringify(payload, null, 2)}\n`);
  return path;
}

function isMainsIdentity(identity: unknown): identity is { firmware: Record<string, unknown> } {
  return (
    !!identity &&
    typeof identity === "object" &&
    "firmware" in identity &&
    !!identity.firmware &&
    typeof identity.firmware === "object" &&
    (identity.firmware as Record<string, unknown>).protocol === "mains-aegis.cdc.v1"
  );
}

function isFirmwareArtifact(value: unknown): value is FirmwareArtifact {
  if (!value || typeof value !== "object") return false;
  const artifact = value as Record<string, unknown>;
  return (
    typeof artifact.artifact_id === "string" &&
    typeof artifact.name === "string" &&
    typeof artifact.version === "string" &&
    typeof artifact.git_sha === "string" &&
    typeof artifact.build_id === "string" &&
    artifact.target_chip === "esp32s3" &&
    isProfile(artifact.profile) &&
    Array.isArray(artifact.features) &&
    artifact.features.every((item) => typeof item === "string") &&
    artifact.protocol === "mains-aegis.cdc.v1" &&
    !!artifact.defmt &&
    typeof artifact.defmt === "object"
  );
}

function isFirmwareFile(value: unknown): value is FirmwareArtifactFile {
  if (!value || typeof value !== "object") return false;
  const file = value as Record<string, unknown>;
  return (
    (file.kind === "elf" || file.kind === "image" || file.kind === "defmt_metadata") &&
    typeof file.path === "string" &&
    typeof file.sha256 === "string" &&
    typeof file.size === "number"
  );
}

function isProfile(value: unknown): value is FirmwareArtifact["profile"] {
  return value === "debug" || value === "release" || value === "dev";
}

function resolveArtifactFilePath(sourceRoot: string | null, path: string): string {
  if (isAbsolute(path)) return path;
  return resolve(sourceRoot ?? repoRoot, path);
}

async function resolveArtifactFile(sourceRoot: string | null, file: FirmwareArtifactFile): Promise<string | null> {
  const direct = resolveArtifactFilePath(sourceRoot, file.path);
  if (await artifactFileMatches(direct, file)) return direct;
  return findMatchingArtifactFile(file);
}

async function findMatchingArtifactFile(file: FirmwareArtifactFile): Promise<string | null> {
  const targetName = basename(file.path);
  for (const path of candidateArtifactPaths(targetName)) {
    if (await artifactFileMatches(path, file)) return path;
  }
  return null;
}

function candidateArtifactPaths(fileName: string): string[] {
  return [
    join(localFirmwareArtifactsRoot, fileName),
    join(firmwareTargetRoot, "xtensa-esp32s3-none-elf/release", fileName),
    join(firmwareTargetRoot, "xtensa-esp32s3-none-elf/debug", fileName),
  ];
}

async function artifactFileMatches(path: string, file: FirmwareArtifactFile): Promise<boolean> {
  const stats = await safeStat(path);
  return !!stats?.isFile() && stats.size === file.size && (await sha256File(path)) === file.sha256;
}

async function sha256File(path: string): Promise<string> {
  return createHash("sha256").update(await readFile(path)).digest("hex");
}

async function safeReadDir(path: string): Promise<string[]> {
  try {
    return await readdir(path);
  } catch {
    return [];
  }
}

async function safeStat(path: string) {
  try {
    return await stat(path);
  } catch {
    return null;
  }
}

async function readJson(path: string): Promise<any | null> {
  try {
    return JSON.parse(await readFile(path, "utf8"));
  } catch {
    return null;
  }
}

async function fetchJsonWithTimeout(url: string, timeoutMs: number): Promise<any | null> {
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), timeoutMs);
  try {
    const response = await fetch(url, { signal: controller.signal, headers: { Accept: "application/json" } });
    if (!response.ok) return null;
    return await response.json();
  } catch {
    return null;
  } finally {
    clearTimeout(timer);
  }
}

function sendJson(res: { statusCode: number; setHeader: (name: string, value: string) => void; end: (body: string) => void }, payload: unknown) {
  res.statusCode = 200;
  res.setHeader("Content-Type", "application/json; charset=utf-8");
  res.setHeader("Cache-Control", "no-store");
  res.end(`${JSON.stringify(payload, null, 2)}\n`);
}

export default defineConfig({
  base: appBase,
  plugins: [devFirmwarePlugin(), react()],
  server: {
    proxy: {
      "/api": {
        target: devdUrl,
        changeOrigin: true,
      },
      "/events": {
        target: devdUrl,
        changeOrigin: true,
      },
    },
  },
});
