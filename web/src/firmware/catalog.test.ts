import { describe, expect, test } from "bun:test";

import type { FirmwareArtifact, Identity } from "../api/types";
import {
  firmwareArtifactFileUrl,
  firmwareArtifactMatchesIdentity,
  firmwareArtifactImageFiles,
  resolveFirmwareCatalogArtifacts,
} from "./catalog";

const artifact: FirmwareArtifact = {
  artifact_id: "mains-aegis-esp32s3-release-web_serial-test",
  name: "mains-aegis",
  version: "0.1.0",
  git_sha: "abcdef0",
  build_id: "abcdef0-clean-source",
  target_chip: "esp32s3",
  profile: "release",
  features: ["web_serial"],
  protocol: "mains-aegis.cdc.v1",
  defmt: {
    enabled: true,
    encoding: "defmt-espflash",
    elf_sha256: null,
    metadata_sha256: null,
  },
  files: [
    { kind: "elf", path: "artifact/esp-firmware", sha256: "0".repeat(64), size: 16 },
    { kind: "image", path: "artifact/merged.bin", sha256: "1".repeat(64), size: 32, flash_address: 0 },
    { kind: "image", path: "artifact/no-address.bin", sha256: "2".repeat(64), size: 32 },
  ],
};

const identity = {
  device_id: "mains-aegis-test",
  hostname: "mains-aegis-test",
  hostname_fqdn: "mains-aegis-test.local",
  short_id: "test",
  role: "ups",
  api_version: "v1",
  firmware: {
    package_version: "0.1.0",
    build_profile: "release",
    build_id: "abcdef0-clean-source",
    git_sha: "abcdef0",
    src_hash: "source",
    git_dirty: "clean",
    features: ["web_serial"],
  },
  network: {
    device_id: "mains-aegis-test",
    hostname: "mains-aegis-test",
    hostname_fqdn: "mains-aegis-test.local",
    state: "disabled",
    ipv4: null,
    gateway: null,
    dns: null,
    is_static: false,
    last_error: null,
    rssi_dbm: null,
  },
  capabilities: {
    sse: true,
    mdns: true,
    dns_sd: true,
    write_controls: true,
  },
} satisfies Identity;

describe("firmware catalog helpers", () => {
  test("matches artifact identity by build, profile, and feature set", () => {
    expect(firmwareArtifactMatchesIdentity(artifact, identity)).toBe(true);
    expect(firmwareArtifactMatchesIdentity({ ...artifact, features: [] }, identity)).toBe(false);
  });

  test("uses only image files with explicit flash addresses for Web Serial flashing", () => {
    expect(firmwareArtifactImageFiles(artifact)).toEqual([
      { kind: "image", path: "artifact/merged.bin", sha256: "1".repeat(64), size: 32, flash_address: 0 },
    ]);
  });

  test("keeps bundled artifacts and overrides duplicate release artifacts", () => {
    const bundled = [{ ...artifact, artifact_id: "shared" }];
    const release = [
      { ...artifact, artifact_id: "shared", build_id: "release-build" },
      { ...artifact, artifact_id: "release-only", build_id: "release-only-build" },
    ];

    const resolution = resolveFirmwareCatalogArtifacts(bundled, release);

    expect(resolution.overridden_release_count).toBe(1);
    expect(resolution.artifacts.map((entry) => entry.artifact.artifact_id)).toEqual(["shared", "release-only"]);
    expect(resolution.artifacts.find((entry) => entry.artifact.artifact_id === "shared")?.source).toBe("bundled_overrides_release");
    expect(resolution.artifacts.find((entry) => entry.artifact.artifact_id === "shared")?.release_duplicate?.build_id).toBe("release-build");
  });

  test("resolves bundled and release artifact file URLs from their source", () => {
    expect(
      firmwareArtifactFileUrl(
        { artifact, source: "bundled", catalog_url: "/firmware/firmware-catalog.json" },
        "artifact/merged.bin",
      ),
    ).toBe("/firmware/artifact/merged.bin");
    expect(
      firmwareArtifactFileUrl(
        {
          artifact,
          source: "github_release",
          catalog_url: "https://example.test/releases/download/v1/firmware-catalog.json",
        },
        "artifact/merged.bin",
      ),
    ).toBe("https://example.test/releases/download/v1/artifact/merged.bin");
  });
});
