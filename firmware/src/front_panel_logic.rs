use crate::front_panel_scene::{
    self, DashboardPrimaryPage, DashboardRoute, SelfCheckUiSnapshot, UiVariant,
};

pub const SELF_CHECK_VARIANT: UiVariant = UiVariant::RetroC;
pub const DASHBOARD_VARIANT: UiVariant = UiVariant::InstrumentB;

pub fn dashboard_uses_frame_animation(
    variant: UiVariant,
    route: DashboardRoute,
    snapshot: &SelfCheckUiSnapshot,
) -> bool {
    variant == DASHBOARD_VARIANT
        && front_panel_scene::dashboard_route_has_active_animation(route, snapshot)
}

pub fn dashboard_enter_requires_variant_switch(variant: UiVariant) -> bool {
    variant != DASHBOARD_VARIANT
}

pub fn dashboard_allowed(snapshot: &SelfCheckUiSnapshot) -> bool {
    !matches!(snapshot.mode, front_panel_scene::UpsMode::Blocked)
        && front_panel_scene::self_check_can_enter_dashboard(snapshot)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VerticalGestureDirection {
    Up,
    Down,
}

pub const fn cst816d_vertical_gesture_direction(raw: u8) -> Option<VerticalGestureDirection> {
    match raw {
        0x01 => Some(VerticalGestureDirection::Up),
        0x02 => Some(VerticalGestureDirection::Down),
        _ => None,
    }
}

pub const fn dashboard_page_for_vertical_menu_gesture(
    page: DashboardPrimaryPage,
    direction: VerticalGestureDirection,
) -> Option<DashboardPrimaryPage> {
    match (page, direction) {
        (DashboardPrimaryPage::DashboardHome, VerticalGestureDirection::Up) => {
            Some(DashboardPrimaryPage::Menu)
        }
        (DashboardPrimaryPage::Menu, VerticalGestureDirection::Down) => {
            Some(DashboardPrimaryPage::DashboardHome)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::net_types::WifiSnapshot;
    use crate::output_state::{EnabledOutputs, OutputSelector};

    fn clear_self_check_snapshot(mode: front_panel_scene::UpsMode) -> SelfCheckUiSnapshot {
        let mut snapshot = SelfCheckUiSnapshot::pending(mode);
        snapshot.gc9307 = front_panel_scene::SelfCheckCommState::Ok;
        snapshot.tca6408a = front_panel_scene::SelfCheckCommState::Ok;
        snapshot.fusb302 = front_panel_scene::SelfCheckCommState::Ok;
        snapshot.ina3221 = front_panel_scene::SelfCheckCommState::Ok;
        snapshot.bq40z50 = front_panel_scene::SelfCheckCommState::Ok;
        snapshot.bq25792 = front_panel_scene::SelfCheckCommState::Ok;
        snapshot.tps_a = front_panel_scene::SelfCheckCommState::Ok;
        snapshot.tps_b = front_panel_scene::SelfCheckCommState::Ok;
        snapshot.tmp_a = front_panel_scene::SelfCheckCommState::Ok;
        snapshot.tmp_b = front_panel_scene::SelfCheckCommState::Ok;
        snapshot.requested_outputs = EnabledOutputs::None;
        snapshot.active_outputs = EnabledOutputs::None;
        snapshot
    }

    #[test]
    fn only_animated_thermal_detail_uses_frame_animation() {
        let mut thermal_active = SelfCheckUiSnapshot::pending(front_panel_scene::UpsMode::Standby);
        thermal_active.dashboard_detail.fan_status = Some("HIGH");
        let thermal_idle = SelfCheckUiSnapshot::pending(front_panel_scene::UpsMode::Standby);

        assert!(dashboard_uses_frame_animation(
            DASHBOARD_VARIANT,
            DashboardRoute::Detail(front_panel_scene::DashboardDetailPage::Thermal),
            &thermal_active,
        ));
        assert!(!dashboard_uses_frame_animation(
            DASHBOARD_VARIANT,
            DashboardRoute::Detail(front_panel_scene::DashboardDetailPage::Thermal),
            &thermal_idle,
        ));
        assert!(!dashboard_uses_frame_animation(
            DASHBOARD_VARIANT,
            DashboardRoute::Detail(front_panel_scene::DashboardDetailPage::Output),
            &thermal_active,
        ));
        assert!(!dashboard_uses_frame_animation(
            DASHBOARD_VARIANT,
            DashboardRoute::Home,
            &thermal_active,
        ));
        assert!(!dashboard_uses_frame_animation(
            UiVariant::RetroC,
            DashboardRoute::Detail(front_panel_scene::DashboardDetailPage::Thermal),
            &thermal_active,
        ));
    }

    #[test]
    fn connecting_wifi_uses_frame_animation_on_dashboard() {
        let mut snapshot = SelfCheckUiSnapshot::pending(front_panel_scene::UpsMode::Standby);
        snapshot.dashboard_detail.wifi = WifiSnapshot::connecting();

        assert!(dashboard_uses_frame_animation(
            DASHBOARD_VARIANT,
            DashboardRoute::Home,
            &snapshot,
        ));
        assert!(dashboard_uses_frame_animation(
            DASHBOARD_VARIANT,
            DashboardRoute::Detail(front_panel_scene::DashboardDetailPage::Wifi),
            &snapshot,
        ));
        assert!(!dashboard_uses_frame_animation(
            UiVariant::RetroC,
            DashboardRoute::Home,
            &snapshot,
        ));
    }

    #[test]
    fn enter_dashboard_only_transitions_from_self_check_variant() {
        assert!(dashboard_enter_requires_variant_switch(SELF_CHECK_VARIANT));
        assert!(!dashboard_enter_requires_variant_switch(DASHBOARD_VARIANT));
    }

    #[test]
    fn dashboard_allowed_accepts_clear_non_blocked_snapshot() {
        let snapshot = clear_self_check_snapshot(front_panel_scene::UpsMode::Standby);

        assert!(dashboard_allowed(&snapshot));
    }

    #[test]
    fn dashboard_allowed_rejects_blocked_mode() {
        let snapshot = clear_self_check_snapshot(front_panel_scene::UpsMode::Blocked);

        assert!(!dashboard_allowed(&snapshot));
    }

    #[test]
    fn dashboard_allowed_rejects_requested_output_without_active_tps() {
        let mut snapshot = clear_self_check_snapshot(front_panel_scene::UpsMode::Standby);
        snapshot.requested_outputs = EnabledOutputs::Only(OutputSelector::OutA);
        snapshot.active_outputs = EnabledOutputs::None;

        assert!(!dashboard_allowed(&snapshot));

        snapshot.active_outputs = EnabledOutputs::Only(OutputSelector::OutA);
        assert!(dashboard_allowed(&snapshot));
    }

    #[test]
    fn cst816d_vertical_gestures_have_direction() {
        assert_eq!(
            cst816d_vertical_gesture_direction(0x01),
            Some(VerticalGestureDirection::Up)
        );
        assert_eq!(
            cst816d_vertical_gesture_direction(0x02),
            Some(VerticalGestureDirection::Down)
        );
        assert_eq!(cst816d_vertical_gesture_direction(0x00), None);
        assert_eq!(cst816d_vertical_gesture_direction(0x03), None);
        assert_eq!(cst816d_vertical_gesture_direction(0x04), None);
    }

    #[test]
    fn vertical_menu_gesture_uses_directional_dashboard_pages() {
        assert_eq!(
            dashboard_page_for_vertical_menu_gesture(
                DashboardPrimaryPage::DashboardHome,
                VerticalGestureDirection::Up
            ),
            Some(DashboardPrimaryPage::Menu)
        );
        assert_eq!(
            dashboard_page_for_vertical_menu_gesture(
                DashboardPrimaryPage::DashboardHome,
                VerticalGestureDirection::Down
            ),
            None
        );
        assert_eq!(
            dashboard_page_for_vertical_menu_gesture(
                DashboardPrimaryPage::Menu,
                VerticalGestureDirection::Down
            ),
            Some(DashboardPrimaryPage::DashboardHome)
        );
        assert_eq!(
            dashboard_page_for_vertical_menu_gesture(
                DashboardPrimaryPage::Menu,
                VerticalGestureDirection::Up
            ),
            None
        );
        assert_eq!(
            dashboard_page_for_vertical_menu_gesture(
                DashboardPrimaryPage::BeeperSettings,
                VerticalGestureDirection::Up
            ),
            None
        );
    }
}
