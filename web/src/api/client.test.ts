import { describe, expect, test } from "bun:test";

import {
  getDeviceAlerts,
  getDeviceChargeControl,
  getDevdDeviceDiagSnapshot,
  getStatus,
  getSettings,
  MainsAegisApiError,
  previewDeviceChargeControl,
  releaseDevdTpsEnableInterlock,
  muteDeviceAlert,
  resetDeviceAdvancedPower,
  setDeviceManualChargeControl,
  setDeviceManualChargePrefs,
  setDeviceAdvancedPower,
  toErrorEnvelope,
} from "./client";
import type { DeviceSettings, UpsStatus } from "./types";

function baseLanStatus(): UpsStatus {
  return {
    mode: "standby",
    input: {
      source: "dcin",
      mains_present: true,
      input_vbus_mv: 12190,
      input_ibus_ma: 274,
      pre_tps_vin_mv: 12190,
      vin_vbus_mv: 12190,
      input_gate_state: "enabled",
      input_gate_reason: "none",
      input_power_good: true,
      vin_iin_ma: 274,
      tps_total_iout_ma: 36,
      tps_limit_threshold_ma: 100,
      pressure_state: "headroom",
      pressure_score_pct: 0,
      pressure_reason: "none",
      vin_baseline_mv: 12190,
      vin_drop_mv: 0,
    },
    output: {
      requested: "both",
      active: "both",
      recoverable: "both",
      gate_reason: "none",
      out_a: {
        state: "ok",
        enabled: true,
        vbus_mv: 11380,
        iout_ma: 16,
      },
      out_b: {
        state: "ok",
        enabled: true,
        vbus_mv: 11380,
        iout_ma: 20,
      },
    },
    charger: {
      state: "ok",
      allow_charge: true,
      ichg_ma: 100,
      ibat_ma: 47,
      vbat_present: true,
      policy_target_ichg_ma: 100,
      limit_active: false,
      limit_reason: "none",
      limit_detail: null,
      limit_threshold_ma: null,
      detail_status: "CHG100",
    },
    charge_control: {
      mode: "auto",
      manual_active: false,
      takeover: false,
      stop_inhibit: false,
      last_stop_reason: null,
      requested_power_path: "auto",
      bound_power_path: "dcin",
      start_state: "ready",
      output_power_w10: 3,
      power_telemetry_fresh: true,
    },
    battery: {
      state: "ok",
      pack_mv: 16020,
      current_ma: 0,
      soc_pct: 99,
      no_battery: false,
      discharge_ready: true,
      charge_fet_on: true,
      discharge_fet_on: true,
      precharge_fet_on: false,
      issue_detail: null,
      recovery_pending: false,
      last_result: null,
    },
    thermal: {
      tmp_a_state: "ok",
      tmp_a_c: 28,
      tmp_b_state: "ok",
      tmp_b_c: 29,
    },
    network: {
      state: "connected",
      ipv4: "192.168.31.232",
      last_error: null,
    },
  };
}

describe("active alerts", () => {
  test("mutes one mock alert instance and returns the authoritative reread", async () => {
    const baseUrl = "mock:alerts-test";
    const before = await getDeviceAlerts(baseUrl);
    const alert = before.alerts.find((item) => item.alert_id === "mains_absent_dc");
    expect(alert?.sound_state).toBe("audible");
    const result = await muteDeviceAlert(baseUrl, alert!.alert_id, alert!.instance_id);
    expect(result).toMatchObject({
      alert_id: alert!.alert_id,
      instance_id: alert!.instance_id,
      severity: "warning",
      sound_state: "muted",
      result: "muted",
    });

    await expect(
      muteDeviceAlert(baseUrl, alert!.alert_id, alert!.instance_id),
    ).resolves.toMatchObject({ result: "already_muted" });
    const after = await getDeviceAlerts(baseUrl);
    expect(after.alerts.find((item) => item.alert_id === alert!.alert_id)?.sound_state).toBe(
      "muted",
    );
    expect(after.alerts.find((item) => item.alert_id === "module_fault")?.sound_state).toBe(
      "system_silent",
    );
  });
});

function baseLanSettings(): DeviceSettings {
  return {
    wifi: {
      configured: true,
      ssid: "lab",
    },
    log_level: "info",
    manual_charge: {
      target: "full_100",
      speed: "ma_500",
      timer_h: 2,
      power_path: "auto",
    },
    charge_capabilities: {
      target_voltage_mv: 16800,
      normal_current_ma: 500,
      dc_derated_current_ma: 100,
      dcin_input_limit_ma: 1000,
      max_output_current_ma: 3500,
      usb_pd_high_power_min_voltage_mv: 9000,
      usb_pd_high_power_max_voltage_mv: 20000,
      usb_pd_high_power_min_power_mw: 20000,
      loop_start_max_power_without_confirm_w10: 20,
      loop_stop_power_latched_w10: 30,
      loop_telemetry_miss_limit: 2,
      supported_power_paths: ["auto", "dcin", "usbc"],
      auto_path_priority: ["usbc", "dcin"],
    },
    advanced_power: {
      standby_drop_mv: 900,
      input_uvlo_cutoff_mv: 11400,
      input_uvlo_recover_mv: 11600,
      input_uvlo_required_samples: 3,
      source_limited_enter_delta_ma: 1000,
    },
    advanced_power_capabilities: {
      rated_vout_mv: 12000,
      standby_drop_mv: {
        default: 900,
        min: 100,
        max: 3000,
        step: 50,
      },
      input_uvlo_cutoff_mv: {
        default: 11400,
        min: 10000,
        max: 13000,
        step: 100,
      },
      input_uvlo_recover_mv: {
        default: 11600,
        min: 10100,
        max: 13100,
        step: 100,
      },
      input_uvlo_required_samples: {
        default: 3,
        min: 1,
        max: 10,
        step: 1,
      },
      source_limited_enter_delta_ma: {
        default: 1000,
        min: 100,
        max: 5000,
        step: 100,
      },
    },
  };
}

function jsonResponse(body: unknown, init?: ResponseInit): Response {
  return new Response(JSON.stringify(body), {
    status: 200,
    headers: {
      "Content-Type": "application/json",
    },
    ...init,
  });
}

async function withFetchMock<T>(
  implementation: typeof fetch,
  run: () => Promise<T>,
): Promise<T> {
  const originalFetch = globalThis.fetch;
  Object.defineProperty(globalThis, "fetch", {
    configurable: true,
    value: implementation,
  });
  try {
    return await run();
  } finally {
    if (originalFetch === undefined) {
      delete (globalThis as typeof globalThis & { fetch?: typeof fetch }).fetch;
    } else {
      Object.defineProperty(globalThis, "fetch", {
        configurable: true,
        value: originalFetch,
      });
    }
  }
}

describe("alert mute errors", () => {
  test("preserves structured Web Serial error envelopes", () => {
    const serialError = Object.assign(new Error("unsupported operation"), {
      envelope: {
        code: "unsupported_operation",
        message: "unsupported operation",
        retryable: false,
        details: null,
      },
    });

    expect(toErrorEnvelope(serialError)).toEqual(serialError.envelope);
  });

  test("maps old firmware direct LAN alert 404 to unsupported", async () => {
    const error = await withFetchMock(
      async () =>
        jsonResponse(
          { error: { code: "not_found", message: "not found" } },
          { status: 404 },
        ),
      async () => {
        try {
          await getDeviceAlerts("http://old-mains-aegis.local");
          throw new Error("expected getDeviceAlerts to reject");
        } catch (caught) {
          return caught;
        }
      },
    );

    expect(error).toBeInstanceOf(MainsAegisApiError);
    expect((error as MainsAegisApiError).envelope).toMatchObject({
      code: "unsupported",
      retryable: false,
      details: { result: "unsupported" },
    });
  });

  test("preserves direct LAN stale results as structured API errors", async () => {
    const error = await withFetchMock(
      async () =>
        jsonResponse(
          {
            ok: false,
            alert_id: "module_fault",
            instance_id: 41,
            result: "stale",
            current_instance_id: 42,
          },
          { status: 409 },
        ),
      async () => {
        try {
          await muteDeviceAlert("http://mains-aegis.local", "module_fault", 41);
          throw new Error("expected muteDeviceAlert to reject");
        } catch (caught) {
          return caught;
        }
      },
    );

    expect(error).toBeInstanceOf(MainsAegisApiError);
    expect((error as MainsAegisApiError).envelope).toMatchObject({
      code: "stale",
      retryable: false,
      details: { result: "stale", current_instance_id: 42 },
    });
  });
});

describe("mock advanced power reset", () => {
  test("preserves 19V mock capabilities when resetting advanced power", async () => {
    const baseUrl = "mock:lab-standby";

    const before = await getSettings(baseUrl);
    expect(before.advanced_power_capabilities.rated_vout_mv).toBe(19000);

    await setDeviceAdvancedPower(baseUrl, {
      standby_drop_mv: 1400,
      input_uvlo_cutoff_mv: 18300,
      input_uvlo_recover_mv: 18500,
      input_uvlo_required_samples: 3,
      source_limited_enter_delta_ma: 1200,
    });

    await resetDeviceAdvancedPower(baseUrl);

    const after = await getSettings(baseUrl);
    expect(after.advanced_power).toEqual({
      standby_drop_mv: 900,
      input_uvlo_cutoff_mv: 18200,
      input_uvlo_recover_mv: 18400,
      input_uvlo_required_samples: 3,
      source_limited_enter_delta_ma: 1000,
    });
    expect(after.advanced_power_capabilities.rated_vout_mv).toBe(19000);
  });

  test("preserves POST bodies for demo HTTP mock targets", async () => {
    const originalWindow = globalThis.window;
    Object.defineProperty(globalThis, "window", {
      configurable: true,
      value: {
        location: new URL("http://localhost/?demo=true"),
      },
    });
    const baseUrl = "http://mains-aegis-a1b2c3.local";

    await setDeviceAdvancedPower(baseUrl, {
      standby_drop_mv: 1550,
      input_uvlo_cutoff_mv: 11400,
      input_uvlo_recover_mv: 11600,
      input_uvlo_required_samples: 4,
      source_limited_enter_delta_ma: 2600,
    });

    const updated = await getSettings(baseUrl);
    expect(updated.advanced_power).toEqual({
      standby_drop_mv: 1550,
      input_uvlo_cutoff_mv: 11400,
      input_uvlo_recover_mv: 11600,
      input_uvlo_required_samples: 4,
      source_limited_enter_delta_ma: 2600,
    });

    if (originalWindow === undefined) {
      delete (globalThis as typeof globalThis & { window?: Window }).window;
    } else {
      Object.defineProperty(globalThis, "window", {
        configurable: true,
        value: originalWindow,
      });
    }
  });

  test("updates mock manual charge prefs and runtime control snapshots", async () => {
    const baseUrl = "mock:lab-standby";

    await setDeviceManualChargePrefs(baseUrl, {
      target: "rsoc_80",
      speed: "ma_500",
      timer_h: 6,
      power_path: "dcin",
    });

    const settings = await getSettings(baseUrl);
    expect(settings.manual_charge).toEqual({
      target: "rsoc_80",
      speed: "ma_500",
      timer_h: 6,
      power_path: "dcin",
    });

    const response = await setDeviceManualChargeControl(baseUrl, {
      action: "start",
    });

    expect(response.summary.manual_active).toBe(true);
    expect(response.readiness.planned_path.bound).toBe("dcin");

    const status = await getStatus(baseUrl);
    expect(status.charge_control?.manual_active).toBe(true);
    expect(status.charge_control?.bound_power_path).toBe("dcin");
  });

  test("surfaces loop confirmation on mock USB-C manual start", async () => {
    const baseUrl = "mock:lab-standby";

    await setDeviceManualChargePrefs(baseUrl, {
      target: "full_100",
      speed: "ma_500",
      timer_h: 2,
      power_path: "usbc",
    });
    const status = await getStatus(baseUrl);
    if (status.charge_control) {
      status.charge_control.output_power_w10 = 24;
    }

    await expect(
      setDeviceManualChargeControl(baseUrl, { action: "start" }),
    ).rejects.toMatchObject({
      envelope: {
        code: "loop_confirmation_required",
        details: {
          readiness: {
            state: "confirm_required",
            planned_path: {
              bound: "usbc",
            },
          },
        },
      },
    });
  });
});

describe("TPS enable interlock release", () => {
  test("provides the MCU runtime latch through the devd mock", async () => {
    const snapshot = await getDevdDeviceDiagSnapshot(
      "mock:devd",
      "mains-aegis-devd-service",
    );

    expect(snapshot.packages["mcu.runtime"]?.payload?.tps_enable_interlock).toEqual({
      therm_kill_n_low: false,
      mcu_drive_low: false,
      tps_en_effective_inhibit: false,
      source: "released",
      asserted_at_ms: null,
      last_release_at_ms: null,
      failure_channel: null,
      failure_stage: null,
      failure_code: null,
    });
  });

  test("reads the live MCU runtime diagnostic package", async () => {
    let requestUrl = "";

    await withFetchMock(
      async (input) => {
        requestUrl = String(input);
        return jsonResponse({
          schema_version: 2,
          packages: {
            "mcu.runtime": {
              payload: {
                tps_enable_interlock: {
                  therm_kill_n_low: false,
                  mcu_drive_low: false,
                  tps_en_effective_inhibit: false,
                  source: "none",
                  asserted_at_ms: null,
                  last_release_at_ms: null,
                },
              },
            },
          },
          errors: {},
        });
      },
      async () =>
        getDevdDeviceDiagSnapshot(
          "http://127.0.0.1:30080",
          "mains-aegis-198840",
        ),
    );

    expect(requestUrl).toBe(
      "http://127.0.0.1:30080/api/v1/devices/mains-aegis-198840/diag-snapshot?package=mcu.runtime",
    );
  });

  test("uses the USB lease and exact confirmation token", async () => {
    let requestUrl = "";
    let requestInit: RequestInit | undefined;

    const result = await withFetchMock(
      async (input, init) => {
        requestUrl = String(input);
        requestInit = init;
        return jsonResponse({
          ok: true,
          accepted: true,
          result: "released",
          mcu_drive_low: false,
          therm_kill_n_low: false,
          warning: null,
          output_gate_reason: "tps_config_failed",
        });
      },
      async () =>
        releaseDevdTpsEnableInterlock(
          "http://127.0.0.1:30080",
          "mains-aegis-198840",
          "usb-lease-1",
        ),
    );

    expect(requestUrl).toBe(
      "http://127.0.0.1:30080/api/v1/devices/mains-aegis-198840/tps-en/release",
    );
    expect(requestInit?.method).toBe("POST");
    expect(JSON.parse(String(requestInit?.body))).toEqual({
      confirm: "release-tps-en",
      lease_id: "usb-lease-1",
    });
    expect(result.result).toBe("released");
  });
});

describe("LAN charge-control compatibility", () => {
  test("falls back to status/settings when the detail endpoint is missing", async () => {
    const status = baseLanStatus();
    const settings = baseLanSettings();

    const detail = await withFetchMock(
      async (input) => {
        const url = String(input);
        if (url.endsWith("/api/v1/charge-control")) {
          return jsonResponse(
            {
              error: {
                code: "not_found",
                message: "not found",
                retryable: false,
                details: null,
              },
            },
            { status: 404, statusText: "Not Found" },
          );
        }
        if (url.endsWith("/api/v1/status")) return jsonResponse(status);
        if (url.endsWith("/api/v1/settings")) return jsonResponse(settings);
        throw new Error(`unexpected fetch: ${url}`);
      },
      async () => getDeviceChargeControl("http://device.test"),
    );

    expect(detail.readiness.state).toBe("ready");
    expect(detail.readiness.planned_path.requested).toBe("auto");
    expect(detail.readiness.planned_path.bound).toBe("dcin");
    expect(detail.telemetry.input_source).toBe("dcin");
    expect(detail.evidence).toEqual(
      expect.arrayContaining([
        expect.objectContaining({
          source: "battery.charge_fet_on",
          value: true,
        }),
      ]),
    );
  });

  test("synthesizes confirm-required preview when preview endpoint is missing", async () => {
    const status = baseLanStatus();
    if (status.charge_control) {
      status.charge_control.output_power_w10 = 24;
      status.charge_control.power_telemetry_fresh = true;
    }
    const settings = baseLanSettings();

    const detail = await withFetchMock(
      async (input) => {
        const url = String(input);
        if (url.endsWith("/api/v1/charge-control/preview")) {
          return jsonResponse(
            {
              error: {
                code: "not_found",
                message: "not found",
                retryable: false,
                details: null,
              },
            },
            { status: 404, statusText: "Not Found" },
          );
        }
        if (url.endsWith("/api/v1/status")) return jsonResponse(status);
        if (url.endsWith("/api/v1/settings")) return jsonResponse(settings);
        throw new Error(`unexpected fetch: ${url}`);
      },
      async () =>
        previewDeviceChargeControl("http://device.test", {
          target: "full_100",
          current_ma: 500,
          timer_minutes: 120,
          power_path: "usbc",
        }),
    );

    expect(detail.readiness.state).toBe("confirm_required");
    expect(detail.readiness.action).toBe("confirm_loop");
    expect(detail.readiness.planned_path.bound).toBe("usbc");
    expect(detail.telemetry.output_power_w10).toBe(24);
  });

  test("converts legacy control responses into charge-control detail", async () => {
    const status = baseLanStatus();
    const settings = baseLanSettings();

    const detail = await withFetchMock(
      async (input) => {
        const url = String(input);
        if (url.endsWith("/api/v1/control/manual-charge")) {
          return jsonResponse({
            charge_control: {
              mode: "manual",
              manual_active: true,
              takeover: false,
              stop_inhibit: false,
              last_stop_reason: null,
              requested_power_path: "dcin",
              bound_power_path: "dcin",
              start_state: "ready",
              output_power_w10: 3,
              power_telemetry_fresh: true,
              remaining_minutes: 119,
              loop_override_active: false,
            },
          });
        }
        if (url.endsWith("/api/v1/status")) return jsonResponse(status);
        if (url.endsWith("/api/v1/settings")) return jsonResponse(settings);
        throw new Error(`unexpected fetch: ${url}`);
      },
      async () =>
        setDeviceManualChargeControl("http://device.test", {
          action: "start",
        }),
    );

    expect(detail.summary.manual_active).toBe(true);
    expect(detail.summary.remaining_minutes).toBe(119);
    expect(detail.readiness.state).toBe("running");
    expect(detail.readiness.planned_path.bound).toBe("dcin");
  });
});
