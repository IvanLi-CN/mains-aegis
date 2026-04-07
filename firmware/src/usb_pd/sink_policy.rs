use super::pd::{PowerDataObject, RequestDataObject, SourceCapabilities};
use super::{ActiveContract, ContractKind, UsbPdPowerDemand};

pub const MAX_SAFE_PD_VOLTAGE_MV: u16 = 20_000;
pub const UNSAFE_INPUT_THRESHOLD_MV: u16 = 20_500;
pub const BQ25792_MIN_VINDPM_MV: u16 = 3_600;
pub const BQ25792_MAX_VINDPM_MV: u16 = 22_000;
pub const BQ25792_MAX_IINDPM_MA: u16 = 3_300;
pub const PPS_HEADROOM_MV: u16 = 600;
pub const PPS_REREQUEST_HYSTERESIS_MV: u16 = 100;
pub const PPS_REREQUEST_MIN_INTERVAL_MS: u32 = 2_000;
const VINDPM_MARGIN_MV: u16 = 1_000;
const PPS_MIN_REQUEST_MV: u16 = 5_000;
const PPS_MAX_REQUEST_MV: u16 = MAX_SAFE_PD_VOLTAGE_MV;
const POWER_EFFICIENCY_NUM: u32 = 115;
const POWER_EFFICIENCY_DEN: u32 = 100;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LocalCapabilities {
    fixed_voltages_mv: [u16; 5],
    fixed_len: usize,
    pub pps_enabled: bool,
}

impl LocalCapabilities {
    pub fn from_features() -> Self {
        let mut fixed_voltages_mv = [0u16; 5];
        let mut fixed_len = 0usize;

        macro_rules! push_fixed {
            ($feature:literal, $mv:expr) => {
                if cfg!(feature = $feature) {
                    fixed_voltages_mv[fixed_len] = $mv;
                    fixed_len += 1;
                }
            };
        }

        push_fixed!("pd-sink-fixed-5v", 5_000);
        push_fixed!("pd-sink-fixed-9v", 9_000);
        push_fixed!("pd-sink-fixed-12v", 12_000);
        push_fixed!("pd-sink-fixed-15v", 15_000);
        push_fixed!("pd-sink-fixed-20v", 20_000);

        Self {
            fixed_voltages_mv,
            fixed_len,
            pps_enabled: cfg!(feature = "pd-sink-pps"),
        }
    }

    #[cfg(test)]
    pub const fn from_parts(
        fixed_voltages_mv: [u16; 5],
        fixed_len: usize,
        pps_enabled: bool,
    ) -> Self {
        Self {
            fixed_voltages_mv,
            fixed_len,
            pps_enabled,
        }
    }

    pub const fn fixed_len(&self) -> usize {
        self.fixed_len
    }

    pub const fn pd_enabled(&self) -> bool {
        self.fixed_len != 0
    }

    pub fn supports_fixed_voltage_mv(&self, voltage_mv: u16) -> bool {
        self.fixed_voltages_mv[..self.fixed_len]
            .iter()
            .copied()
            .any(|supported_mv| supported_mv == voltage_mv)
    }

    pub fn fixed_voltages_mv(&self) -> &[u16] {
        &self.fixed_voltages_mv[..self.fixed_len]
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FixedSourceOffer {
    pub object_position: u8,
    pub voltage_mv: u16,
    pub max_current_ma: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PpsSourceOffer {
    pub object_position: u8,
    pub min_voltage_mv: u16,
    pub max_voltage_mv: u16,
    pub max_current_ma: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceOffer {
    Unsupported,
    Fixed(FixedSourceOffer),
    Pps(PpsSourceOffer),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FilteredSourceCapabilities {
    offers: [SourceOffer; 7],
    len: usize,
}

impl FilteredSourceCapabilities {
    pub const fn empty() -> Self {
        Self {
            offers: [SourceOffer::Unsupported; 7],
            len: 0,
        }
    }

    pub const fn len(&self) -> usize {
        self.len
    }

    pub fn get(&self, index: usize) -> Option<SourceOffer> {
        (index < self.len).then_some(self.offers[index])
    }

    pub fn iter(&self) -> impl Iterator<Item = SourceOffer> + '_ {
        self.offers[..self.len].iter().copied()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ContractPlan {
    pub contract: ActiveContract,
    pub request: RequestDataObject,
}

pub fn filter_source_capabilities(source: &SourceCapabilities) -> FilteredSourceCapabilities {
    let mut filtered = FilteredSourceCapabilities::empty();

    for (index, pdo) in source.iter() {
        let object_position = (index + 1) as u8;
        match pdo {
            PowerDataObject::FixedSupply(pdo) if pdo.voltage_mv <= MAX_SAFE_PD_VOLTAGE_MV => {
                filtered.offers[filtered.len] = SourceOffer::Fixed(FixedSourceOffer {
                    object_position,
                    voltage_mv: pdo.voltage_mv,
                    max_current_ma: pdo.max_current_ma,
                });
                filtered.len += 1;
            }
            PowerDataObject::Pps(apdo)
                if apdo.min_voltage_mv <= MAX_SAFE_PD_VOLTAGE_MV
                    && apdo.max_voltage_mv <= MAX_SAFE_PD_VOLTAGE_MV =>
            {
                filtered.offers[filtered.len] = SourceOffer::Pps(PpsSourceOffer {
                    object_position,
                    min_voltage_mv: apdo.min_voltage_mv,
                    max_voltage_mv: apdo.max_voltage_mv,
                    max_current_ma: apdo.max_current_ma,
                });
                filtered.len += 1;
            }
            _ => {}
        }
    }

    filtered
}

pub fn select_contract(
    local: &LocalCapabilities,
    source: &SourceCapabilities,
    demand: UsbPdPowerDemand,
) -> Option<ContractPlan> {
    let filtered = filter_source_capabilities(source);
    if filtered.len() == 0 || !local.pd_enabled() {
        return None;
    }

    if local.pps_enabled {
        if let Some(plan) = select_pps_contract(&filtered, demand) {
            return Some(plan);
        }
    }

    select_fixed_contract(local, &filtered, demand)
}

pub fn select_fixed_contract(
    local: &LocalCapabilities,
    filtered: &FilteredSourceCapabilities,
    demand: UsbPdPowerDemand,
) -> Option<ContractPlan> {
    let required_power_mw = demand.required_power_mw();
    let mut best_offer: Option<FixedSourceOffer> = None;
    let mut best_required_current_ma = 0u16;

    for offer in filtered.iter() {
        let SourceOffer::Fixed(offer) = offer else {
            continue;
        };
        if !local.supports_fixed_voltage_mv(offer.voltage_mv) {
            continue;
        }

        let required_current_ma = required_input_current_ma(required_power_mw, offer.voltage_mv);
        if offer.max_current_ma < required_current_ma {
            continue;
        }

        let take = match best_offer {
            None => true,
            Some(current_best) => {
                offer.voltage_mv < current_best.voltage_mv
                    || (offer.voltage_mv == current_best.voltage_mv
                        && offer.max_current_ma > current_best.max_current_ma)
            }
        };
        if take {
            best_offer = Some(offer);
            best_required_current_ma = required_current_ma;
        }
    }

    best_offer.map(|offer| {
        let current_ma = best_required_current_ma.max(100).min(offer.max_current_ma);
        let vindpm_mv = compute_vindpm_mv(offer.voltage_mv);
        ContractPlan {
            contract: ActiveContract {
                kind: ContractKind::Fixed,
                object_position: offer.object_position,
                voltage_mv: offer.voltage_mv,
                current_ma,
                source_max_current_ma: offer.max_current_ma,
                input_current_limit_ma: Some(current_ma.min(BQ25792_MAX_IINDPM_MA)),
                vindpm_mv: Some(vindpm_mv),
            },
            request: RequestDataObject::for_fixed(offer.object_position, current_ma),
        }
    })
}

pub fn select_pps_contract(
    filtered: &FilteredSourceCapabilities,
    demand: UsbPdPowerDemand,
) -> Option<ContractPlan> {
    let required_power_mw = demand.required_power_mw();
    let mut best_offer: Option<PpsSourceOffer> = None;
    let mut best_voltage_error_mv = u16::MAX;
    let target_voltage_mv = compute_pps_target_voltage_mv(demand);

    for offer in filtered.iter() {
        let SourceOffer::Pps(offer) = offer else {
            continue;
        };
        let requested_voltage_mv = clamp_u16(
            target_voltage_mv,
            offer.min_voltage_mv,
            offer.max_voltage_mv,
        );
        let required_current_ma =
            required_input_current_ma(required_power_mw, requested_voltage_mv);
        if offer.max_current_ma < required_current_ma {
            continue;
        }
        let error_mv = requested_voltage_mv.abs_diff(target_voltage_mv);
        let take = match best_offer {
            None => true,
            Some(current_best) => {
                error_mv < best_voltage_error_mv
                    || (error_mv == best_voltage_error_mv
                        && offer.max_voltage_mv < current_best.max_voltage_mv)
            }
        };
        if take {
            best_offer = Some(offer);
            best_voltage_error_mv = error_mv;
        }
    }

    best_offer.map(|offer| {
        let requested_voltage_mv = clamp_u16(
            target_voltage_mv,
            offer.min_voltage_mv,
            offer.max_voltage_mv,
        );
        let current_ma = required_input_current_ma(required_power_mw, requested_voltage_mv)
            .max(100)
            .min(offer.max_current_ma)
            .min(BQ25792_MAX_IINDPM_MA);
        ContractPlan {
            contract: ActiveContract {
                kind: ContractKind::Pps,
                object_position: offer.object_position,
                voltage_mv: requested_voltage_mv,
                current_ma,
                source_max_current_ma: offer.max_current_ma,
                input_current_limit_ma: Some(current_ma),
                vindpm_mv: Some(compute_vindpm_mv(requested_voltage_mv)),
            },
            request: RequestDataObject::for_pps(
                offer.object_position,
                requested_voltage_mv,
                current_ma,
            ),
        }
    })
}

pub fn should_refresh_pps_contract(
    current_contract: ActiveContract,
    next_contract: ActiveContract,
    now_ms: u32,
    last_request_at_ms: u32,
) -> bool {
    if current_contract.kind != ContractKind::Pps || next_contract.kind != ContractKind::Pps {
        return false;
    }
    if now_ms.wrapping_sub(last_request_at_ms) < PPS_REREQUEST_MIN_INTERVAL_MS {
        return false;
    }

    current_contract
        .voltage_mv
        .abs_diff(next_contract.voltage_mv)
        >= PPS_REREQUEST_HYSTERESIS_MV
        || current_contract
            .current_ma
            .abs_diff(next_contract.current_ma)
            >= 100
}

pub const fn is_input_voltage_unsafe(measured_input_voltage_mv: Option<u16>) -> bool {
    matches!(measured_input_voltage_mv, Some(mv) if mv > UNSAFE_INPUT_THRESHOLD_MV)
}

pub fn compute_pps_target_voltage_mv(demand: UsbPdPowerDemand) -> u16 {
    let desired_mv = if demand.requested_charge_voltage_mv != 0 {
        demand
            .requested_charge_voltage_mv
            .saturating_add(PPS_HEADROOM_MV)
    } else if let Some(battery_voltage_mv) = demand.battery_voltage_mv {
        battery_voltage_mv.saturating_add(PPS_HEADROOM_MV)
    } else {
        PPS_MIN_REQUEST_MV
    };

    clamp_u16(desired_mv, PPS_MIN_REQUEST_MV, PPS_MAX_REQUEST_MV)
}

pub fn required_input_current_ma(power_mw: u32, input_voltage_mv: u16) -> u16 {
    if power_mw == 0 || input_voltage_mv == 0 {
        return 100;
    }

    let adjusted_power_mw = power_mw.saturating_mul(POWER_EFFICIENCY_NUM) / POWER_EFFICIENCY_DEN;
    let current_ma = (adjusted_power_mw + input_voltage_mv as u32 - 1) / input_voltage_mv as u32;
    current_ma.min(u16::MAX as u32) as u16
}

pub const fn compute_vindpm_mv(contract_voltage_mv: u16) -> u16 {
    clamp_u16(
        contract_voltage_mv.saturating_sub(VINDPM_MARGIN_MV),
        BQ25792_MIN_VINDPM_MV,
        BQ25792_MAX_VINDPM_MV,
    )
}

const fn clamp_u16(value: u16, min: u16, max: u16) -> u16 {
    if value < min {
        min
    } else if value > max {
        max
    } else {
        value
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::usb_pd::pd::{
        DataMessageType, Message, MessageHeader, PowerDataObject, SpecRevision,
    };

    const LOCAL_FIXED_ONLY: LocalCapabilities =
        LocalCapabilities::from_parts([5_000, 9_000, 12_000, 0, 0], 3, false);
    const LOCAL_FIXED_AND_PPS: LocalCapabilities =
        LocalCapabilities::from_parts([5_000, 9_000, 12_000, 15_000, 20_000], 5, true);

    fn fixed_pdo_raw(voltage_mv: u16, current_ma: u16) -> u32 {
        (((voltage_mv / crate::usb_pd::pd::FIXED_VOLTAGE_STEP_MV) as u32) << 10)
            | ((current_ma / 10) as u32)
    }

    fn pps_apdo_raw(min_mv: u16, max_mv: u16, current_ma: u16) -> u32 {
        (3u32 << 30)
            | (((max_mv / 100) as u32) << 17)
            | (((min_mv / 100) as u32) << 8)
            | ((current_ma / 50) as u32)
    }

    fn build_source_caps(objects: [u32; 7], count: usize) -> SourceCapabilities {
        let header = MessageHeader::for_data(
            DataMessageType::SourceCapabilities,
            count,
            0,
            SpecRevision::Rev30,
            true,
            false,
        );
        SourceCapabilities::from_message(&Message::new(header, objects)).unwrap()
    }

    #[test]
    fn filters_out_over_20v_fixed_and_pps_offers() {
        let caps = build_source_caps(
            [
                fixed_pdo_raw(5_000, 3_000),
                fixed_pdo_raw(20_000, 3_000),
                fixed_pdo_raw(28_000, 2_000),
                pps_apdo_raw(5_000, 11_000, 3_000),
                pps_apdo_raw(5_000, 21_000, 3_000),
                0,
                0,
            ],
            5,
        );

        let filtered = filter_source_capabilities(&caps);
        assert_eq!(filtered.len(), 3);
        assert!(matches!(filtered.get(0), Some(SourceOffer::Fixed(_))));
        assert!(matches!(filtered.get(1), Some(SourceOffer::Fixed(_))));
        assert!(matches!(filtered.get(2), Some(SourceOffer::Pps(_))));
    }

    #[test]
    fn selects_lowest_fixed_voltage_that_satisfies_power() {
        let caps = build_source_caps(
            [
                fixed_pdo_raw(5_000, 3_000),
                fixed_pdo_raw(9_000, 2_000),
                fixed_pdo_raw(12_000, 1_500),
                0,
                0,
                0,
                0,
            ],
            3,
        );
        let demand = UsbPdPowerDemand {
            requested_charge_voltage_mv: 16_800,
            requested_charge_current_ma: 500,
            battery_voltage_mv: Some(14_800),
            measured_input_voltage_mv: None,
            charging_enabled: true,
        };

        let plan = select_contract(&LOCAL_FIXED_ONLY, &caps, demand).unwrap();
        assert_eq!(plan.contract.kind, ContractKind::Fixed);
        assert_eq!(plan.contract.voltage_mv, 5_000);
    }

    #[test]
    fn selects_pps_when_enabled_and_offer_is_valid() {
        let caps = build_source_caps(
            [
                fixed_pdo_raw(5_000, 3_000),
                pps_apdo_raw(5_000, 18_000, 3_000),
                0,
                0,
                0,
                0,
                0,
            ],
            2,
        );
        let demand = UsbPdPowerDemand {
            requested_charge_voltage_mv: 16_800,
            requested_charge_current_ma: 500,
            battery_voltage_mv: Some(15_200),
            measured_input_voltage_mv: None,
            charging_enabled: true,
        };

        let plan = select_contract(&LOCAL_FIXED_AND_PPS, &caps, demand).unwrap();
        assert_eq!(plan.contract.kind, ContractKind::Pps);
        assert_eq!(plan.contract.voltage_mv, 17_400);
        assert_eq!(plan.request.pps_voltage_mv(), 17_400);
    }

    #[test]
    fn pps_refresh_requires_hysteresis_and_interval() {
        let current = ActiveContract {
            kind: ContractKind::Pps,
            object_position: 2,
            voltage_mv: 8_000,
            current_ma: 2_000,
            source_max_current_ma: 3_000,
            input_current_limit_ma: Some(2_000),
            vindpm_mv: Some(7_000),
        };
        let next = ActiveContract {
            voltage_mv: 8_080,
            ..current
        };
        assert!(!should_refresh_pps_contract(current, next, 5_000, 2_000));

        let next = ActiveContract {
            voltage_mv: 8_200,
            ..current
        };
        assert!(!should_refresh_pps_contract(current, next, 3_500, 2_000));
        assert!(should_refresh_pps_contract(current, next, 4_500, 2_000));
    }

    #[test]
    fn detects_unsafe_input_voltage() {
        assert!(!is_input_voltage_unsafe(Some(20_500)));
        assert!(is_input_voltage_unsafe(Some(20_501)));
        assert!(!is_input_voltage_unsafe(None));
    }

    #[test]
    fn computes_reasonable_vindpm_limit() {
        assert_eq!(compute_vindpm_mv(5_000), 4_000);
        assert_eq!(compute_vindpm_mv(20_000), 19_000);
    }

    #[test]
    fn source_cap_builder_keeps_supported_objects_copyable() {
        let caps = build_source_caps([fixed_pdo_raw(9_000, 2_000), 0, 0, 0, 0, 0, 0], 1);
        match caps.get(0).unwrap() {
            PowerDataObject::FixedSupply(pdo) => assert_eq!(pdo.voltage_mv, 9_000),
            other => panic!("unexpected PDO: {other:?}"),
        }
    }
}
