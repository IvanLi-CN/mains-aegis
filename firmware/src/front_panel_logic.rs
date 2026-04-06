use crate::front_panel_scene::{self, DashboardRoute, SelfCheckUiSnapshot, UiVariant};

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

#[cfg(test)]
mod tests {
    use super::*;

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
    fn enter_dashboard_only_transitions_from_self_check_variant() {
        assert!(dashboard_enter_requires_variant_switch(SELF_CHECK_VARIANT));
        assert!(!dashboard_enter_requires_variant_switch(DASHBOARD_VARIANT));
    }
}
