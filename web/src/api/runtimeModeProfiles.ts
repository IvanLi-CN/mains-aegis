import runtimeModeProfilesJson from "../../../schemas/runtime_mode_profiles.json";
import type { DeviceSettings, UpsStatus } from "./types";

type RuntimeModeProfilesFile = {
  capabilities: {
    standby_drop_mv: { min: number; max: number; step: number };
    input_uvlo_threshold_mv: { min: number; max: number; step: number };
    input_uvlo_required_samples: { min: number; max: number; step: number };
    source_limited_enter_delta_ma: { min: number; max: number; step: number };
  };
  profiles: Array<{
    max_rated_vout_mv: number;
    defaults: DeviceSettings["advanced_power"];
  }>;
};

const runtimeModeProfiles = runtimeModeProfilesJson as RuntimeModeProfilesFile;

export function resolveRuntimeModeProfile(ratedVoutMv: number) {
  return (
    runtimeModeProfiles.profiles.find(
      (profile) => ratedVoutMv <= profile.max_rated_vout_mv,
    ) ?? runtimeModeProfiles.profiles[runtimeModeProfiles.profiles.length - 1]
  );
}

export function buildAdvancedPowerDefaults(
  ratedVoutMv: number,
): DeviceSettings["advanced_power"] {
  return { ...resolveRuntimeModeProfile(ratedVoutMv).defaults };
}

export function buildAdvancedPowerCapabilities(
  ratedVoutMv: number,
): DeviceSettings["advanced_power_capabilities"] {
  const defaults = buildAdvancedPowerDefaults(ratedVoutMv);
  const bounds = runtimeModeProfiles.capabilities;
  return {
    rated_vout_mv: ratedVoutMv,
    standby_drop_mv: {
      default: defaults.standby_drop_mv,
      min: bounds.standby_drop_mv.min,
      max: bounds.standby_drop_mv.max,
      step: bounds.standby_drop_mv.step,
    },
    input_uvlo_cutoff_mv: {
      default: defaults.input_uvlo_cutoff_mv,
      min: bounds.input_uvlo_threshold_mv.min,
      max: bounds.input_uvlo_threshold_mv.max,
      step: bounds.input_uvlo_threshold_mv.step,
    },
    input_uvlo_recover_mv: {
      default: defaults.input_uvlo_recover_mv,
      min: bounds.input_uvlo_threshold_mv.min,
      max: bounds.input_uvlo_threshold_mv.max,
      step: bounds.input_uvlo_threshold_mv.step,
    },
    input_uvlo_required_samples: {
      default: defaults.input_uvlo_required_samples,
      min: bounds.input_uvlo_required_samples.min,
      max: bounds.input_uvlo_required_samples.max,
      step: bounds.input_uvlo_required_samples.step,
    },
    source_limited_enter_delta_ma: {
      default: defaults.source_limited_enter_delta_ma,
      min: bounds.source_limited_enter_delta_ma.min,
      max: bounds.source_limited_enter_delta_ma.max,
      step: bounds.source_limited_enter_delta_ma.step,
    },
  };
}

export function resolvePreTpsVinMv(
  input: Pick<UpsStatus["input"], "pre_tps_vin_mv" | "vin_vbus_mv"> | null | undefined,
): number | null | undefined {
  return input?.pre_tps_vin_mv ?? input?.vin_vbus_mv;
}
