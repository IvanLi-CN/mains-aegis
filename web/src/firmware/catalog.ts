import { loadBundledFirmwareCatalog } from "../api/client";
import type { FirmwareArtifact, FirmwareArtifactFile, FirmwareArtifactMatch, FirmwareCatalog, Identity } from "../api/types";

export const BUNDLED_FIRMWARE_CATALOG_URL = "/firmware/firmware-catalog.json";
export const DEFAULT_GITHUB_FIRMWARE_CATALOG_URL = "github-release:IvanLi-CN/mains-aegis";
export const GITHUB_FIRMWARE_CATALOG_URL = import.meta.env.VITE_FIRMWARE_CATALOG_URL ?? DEFAULT_GITHUB_FIRMWARE_CATALOG_URL;

export type FirmwareCatalogSource = FirmwareArtifactMatch["source"];

export type ResolvedFirmwareArtifact = FirmwareArtifactMatch & {
  release_duplicate?: FirmwareArtifact;
  manifest_path?: string;
};

export type FirmwareCatalogResolution = {
  artifacts: ResolvedFirmwareArtifact[];
  source_status: {
    bundled: "loaded" | "error";
    github_release: "loaded" | "skipped" | "error";
  };
  overridden_release_count: number;
};

export async function loadResolvedFirmwareCatalog(): Promise<FirmwareCatalogResolution> {
  let bundledStatus: FirmwareCatalogResolution["source_status"]["bundled"] = "loaded";
  let releaseStatus: FirmwareCatalogResolution["source_status"]["github_release"] = GITHUB_FIRMWARE_CATALOG_URL.trim() ? "loaded" : "skipped";
  let bundledArtifacts: FirmwareArtifact[] = [];
  let releaseArtifacts: FirmwareArtifact[] = [];
  let releaseCatalogUrl = GITHUB_FIRMWARE_CATALOG_URL;

  try {
    bundledArtifacts = (await loadBundledFirmwareCatalog()).artifacts;
  } catch {
    bundledStatus = "error";
  }

  if (GITHUB_FIRMWARE_CATALOG_URL.trim()) {
    try {
      const releaseCatalog = await loadReleaseFirmwareCatalog(GITHUB_FIRMWARE_CATALOG_URL, 1800);
      releaseArtifacts = releaseCatalog.catalog.artifacts;
      releaseCatalogUrl = releaseCatalog.catalog_url;
    } catch {
      releaseStatus = "error";
    }
  }

  const resolution = resolveFirmwareCatalogArtifacts(bundledArtifacts, releaseArtifacts, releaseCatalogUrl);

  return {
    artifacts: resolution.artifacts,
    source_status: {
      bundled: bundledStatus,
      github_release: releaseStatus,
    },
    overridden_release_count: resolution.overridden_release_count,
  };
}

async function loadFirmwareCatalogFromUrlWithTimeout(url: string, timeoutMs: number): Promise<FirmwareCatalog> {
  const controller = new AbortController();
  const timer = window.setTimeout(() => controller.abort(), timeoutMs);
  try {
    const response = await fetch(url, {
      signal: controller.signal,
      headers: {
        Accept: "application/json",
      },
    });
    if (!response.ok) {
      throw new Error(`firmware catalog request failed with ${response.status}`);
    }
    return (await response.json()) as FirmwareCatalog;
  } finally {
    window.clearTimeout(timer);
  }
}

async function loadReleaseFirmwareCatalog(url: string, timeoutMs: number): Promise<{ catalog: FirmwareCatalog; catalog_url: string }> {
  const releaseRef = parseGitHubReleaseRef(url);
  if (!releaseRef) {
    return { catalog: await loadFirmwareCatalogFromUrlWithTimeout(url, timeoutMs), catalog_url: url };
  }

  const release = await loadGitHubLatestRelease(releaseRef.owner, releaseRef.repo, timeoutMs);
  const asset = release.assets.find((candidate) => candidate.name === "firmware-catalog.json");
  if (!asset) throw new Error("firmware-catalog.json release asset not found");
  const catalog = await loadGitHubReleaseAssetJson(asset.url, timeoutMs);
  return { catalog, catalog_url: asset.browser_download_url };
}

export function resolveFirmwareCatalogArtifacts(
  bundledArtifacts: FirmwareArtifact[],
  releaseArtifacts: FirmwareArtifact[],
  releaseCatalogUrl = GITHUB_FIRMWARE_CATALOG_URL,
): Pick<FirmwareCatalogResolution, "artifacts" | "overridden_release_count"> {
  const artifacts = new Map<string, ResolvedFirmwareArtifact>();
  let overriddenReleaseCount = 0;

  for (const artifact of bundledArtifacts) {
    artifacts.set(artifact.artifact_id, {
      artifact,
      source: "bundled",
      catalog_url: BUNDLED_FIRMWARE_CATALOG_URL,
      manifest_path: artifact.devd_manifest_path ?? `web/public/firmware/${artifact.artifact_id}.manifest.json`,
    });
  }

  for (const artifact of releaseArtifacts) {
    const existing = artifacts.get(artifact.artifact_id);
    if (existing) {
      overriddenReleaseCount += 1;
      artifacts.set(artifact.artifact_id, {
        ...existing,
        source: "bundled_overrides_release",
        release_duplicate: artifact,
        manifest_path: existing.manifest_path,
      });
      continue;
    }
    artifacts.set(artifact.artifact_id, {
      artifact,
      source: "github_release",
      catalog_url: releaseCatalogUrl,
    });
  }

  return {
    artifacts: Array.from(artifacts.values()).sort(compareResolvedArtifacts),
    overridden_release_count: overriddenReleaseCount,
  };
}

type GitHubReleaseAsset = {
  name: string;
  url: string;
  browser_download_url: string;
};

type GitHubRelease = {
  assets: GitHubReleaseAsset[];
};

function parseGitHubReleaseRef(url: string): { owner: string; repo: string } | null {
  if (url.startsWith("github-release:")) {
    const [owner, repo] = url.slice("github-release:".length).split("/");
    return owner && repo ? { owner, repo } : null;
  }
  const match = url.match(/^https:\/\/github\.com\/([^/]+)\/([^/]+)\/releases\/latest\/download\/firmware-catalog\.json$/);
  return match ? { owner: match[1], repo: match[2] } : null;
}

async function loadGitHubLatestRelease(owner: string, repo: string, timeoutMs: number): Promise<GitHubRelease> {
  return fetchJsonWithTimeout(`https://api.github.com/repos/${owner}/${repo}/releases/latest`, timeoutMs, {
    Accept: "application/vnd.github+json",
  });
}

async function loadGitHubReleaseAssetJson(assetApiUrl: string, timeoutMs: number): Promise<FirmwareCatalog> {
  return fetchJsonWithTimeout(assetApiUrl, timeoutMs, {
    Accept: "application/octet-stream",
  });
}

async function fetchJsonWithTimeout<T>(url: string, timeoutMs: number, headers: Record<string, string>): Promise<T> {
  const controller = new AbortController();
  const timer = window.setTimeout(() => controller.abort(), timeoutMs);
  try {
    const response = await fetch(url, {
      signal: controller.signal,
      headers,
    });
    if (!response.ok) throw new Error(`request failed with ${response.status}`);
    return (await response.json()) as T;
  } finally {
    window.clearTimeout(timer);
  }
}

export async function findFirmwareArtifactForIdentity(identity: Identity): Promise<FirmwareArtifactMatch | null> {
  const resolution = await loadResolvedFirmwareCatalog();
  return resolution.artifacts.find((candidate) => firmwareArtifactMatchesIdentity(candidate.artifact, identity)) ?? null;
}

export async function findBundledFirmwareArtifact(identity: Identity): Promise<FirmwareArtifact | null> {
  try {
    const catalog = await loadBundledFirmwareCatalog();
    return catalog.artifacts.find((artifact) => firmwareArtifactMatchesIdentity(artifact, identity)) ?? null;
  } catch {
    return null;
  }
}

export function firmwareCatalogSourceLabel(source: FirmwareCatalogSource): string {
  if (source === "bundled_overrides_release") return "Bundled";
  return source === "github_release" ? "GitHub Release" : "Bundled";
}

export function firmwareArtifactMatchesIdentity(artifact: FirmwareArtifact, identity: Identity): boolean {
  return (
    artifact.build_id === identity.firmware.build_id &&
    artifact.profile === identity.firmware.build_profile &&
    sameStringSet(artifact.features, identity.firmware.features ?? [])
  );
}

export function firmwareArtifactElfPath(artifact: FirmwareArtifact): string | null {
  const file = artifact.files.find((candidate) => candidate.kind === "elf");
  return file ? `/firmware/${file.path}` : null;
}

export function firmwareArtifactImageFiles(artifact: FirmwareArtifact): Array<FirmwareArtifactFile & { kind: "image"; flash_address: number }> {
  return artifact.files.filter(isFlashImageFile);
}

export function firmwareArtifactHasWebFlashImages(artifact: FirmwareArtifact): boolean {
  return firmwareArtifactImageFiles(artifact).length > 0;
}

export function firmwareArtifactFileUrl(entry: FirmwareArtifactMatch, path: string): string {
  if (entry.source === "bundled" || entry.source === "bundled_overrides_release") {
    return `/firmware/${path}`;
  }
  return new URL(path, entry.catalog_url).toString();
}

function isFlashImageFile(file: FirmwareArtifactFile): file is FirmwareArtifactFile & { kind: "image"; flash_address: number } {
  return file.kind === "image" && Number.isInteger(file.flash_address) && (file.flash_address ?? -1) >= 0;
}

function compareResolvedArtifacts(left: ResolvedFirmwareArtifact, right: ResolvedFirmwareArtifact): number {
  const leftRank = sourceRank(left.source);
  const rightRank = sourceRank(right.source);
  if (leftRank !== rightRank) return leftRank - rightRank;
  return left.artifact.artifact_id.localeCompare(right.artifact.artifact_id);
}

function sourceRank(source: FirmwareCatalogSource): number {
  if (source === "bundled" || source === "bundled_overrides_release") return 0;
  return 1;
}

function sameStringSet(left: string[], right: string[]): boolean {
  if (left.length !== right.length) return false;
  const sortedLeft = [...left].sort();
  const sortedRight = [...right].sort();
  return sortedLeft.every((value, index) => value === sortedRight[index]);
}
