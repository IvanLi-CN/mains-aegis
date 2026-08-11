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

pub const fn map_cst816d_touch_to_landscape_swapped(x_raw: u16, y_raw: u16) -> Option<(u16, u16)> {
    let ui_w = front_panel_scene::UI_W;
    let ui_h = front_panel_scene::UI_H;

    if x_raw < ui_h && y_raw < ui_w {
        return Some((ui_w - 1 - y_raw, x_raw));
    }

    // Retain the legacy landscape ordering for older touch-controller setup.
    if x_raw < ui_w && y_raw < ui_h {
        return Some((x_raw, y_raw));
    }
    None
}

pub fn dashboard_header_entry_target(
    alerts_available: bool,
    previous: Option<(u16, u16)>,
    current: (u16, u16),
) -> Option<front_panel_scene::DashboardHomeTouchTarget> {
    use front_panel_scene::{DashboardHomeTouchTarget, DashboardTouchTarget};

    let target =
        front_panel_scene::dashboard_home_hit_test(alerts_available, current.0, current.1)?;
    if !matches!(
        target,
        DashboardHomeTouchTarget::Alerts
            | DashboardHomeTouchTarget::Dashboard(DashboardTouchTarget::HomeWifi)
    ) {
        return None;
    }
    if previous.is_some_and(|(x, y)| {
        front_panel_scene::dashboard_home_hit_test(alerts_available, x, y) == Some(target)
    }) {
        None
    } else {
        Some(target)
    }
}

pub const fn any_alert_button_edge(
    up: bool,
    down: bool,
    left: bool,
    right: bool,
    center: bool,
) -> bool {
    up || down || left || right || center
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

    #[test]
    fn landscape_swapped_touch_mapping_has_explicit_raw_boundaries() {
        assert_eq!(map_cst816d_touch_to_landscape_swapped(0, 0), Some((319, 0)));
        assert_eq!(
            map_cst816d_touch_to_landscape_swapped(171, 319),
            Some((0, 171))
        );

        // Legacy landscape ordering remains accepted, but is bounded to 320x172.
        assert_eq!(
            map_cst816d_touch_to_landscape_swapped(319, 171),
            Some((319, 171))
        );
        assert_eq!(map_cst816d_touch_to_landscape_swapped(320, 171), None);
        assert_eq!(map_cst816d_touch_to_landscape_swapped(319, 172), None);
        assert_eq!(
            map_cst816d_touch_to_landscape_swapped(u16::MAX, u16::MAX),
            None
        );
    }

    #[test]
    fn dashboard_header_targets_trigger_on_press_or_entry_only() {
        use front_panel_scene::{DashboardHomeTouchTarget, DashboardTouchTarget};

        let wifi = DashboardHomeTouchTarget::Dashboard(DashboardTouchTarget::HomeWifi);
        assert_eq!(
            dashboard_header_entry_target(true, None, (112, 0)),
            Some(wifi)
        );
        assert_eq!(
            dashboard_header_entry_target(true, Some((100, 30)), (149, 35)),
            Some(wifi)
        );
        assert_eq!(
            dashboard_header_entry_target(true, Some((112, 0)), (149, 35)),
            None
        );
        assert_eq!(
            dashboard_header_entry_target(true, Some((149, 35)), (150, 35)),
            Some(DashboardHomeTouchTarget::Alerts)
        );
        assert_eq!(
            dashboard_header_entry_target(false, Some((149, 35)), (150, 35)),
            None
        );
        assert_eq!(
            dashboard_header_entry_target(true, Some((100, 30)), (100, 60)),
            None
        );
    }

    #[test]
    fn alert_buttons_do_not_repeat_feedback_without_a_new_edge() {
        assert!(!any_alert_button_edge(false, false, false, false, false));
        assert!(any_alert_button_edge(true, false, false, false, false));
        assert!(any_alert_button_edge(false, false, false, true, false));
        assert!(any_alert_button_edge(false, false, false, false, true));
    }
}
