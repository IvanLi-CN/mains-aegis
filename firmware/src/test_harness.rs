use crate::front_panel_scene::{
    test_audio_play_hit_test, test_audio_stop_hit_test, test_back_hit_test,
    test_navigation_hit_test, TestFunctionUi,
};
use crate::test_audio::AudioEvent;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TestFunction {
    ScreenStatic,
    AudioPlayback,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TestRoute {
    Navigation,
    ScreenStatic,
    AudioPlayback,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HarnessInputEvent {
    Up,
    Down,
    Left,
    Right,
    Center,
    Touch { x: u16, y: u16 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HarnessResult {
    pub needs_redraw: bool,
    pub audio_event: Option<AudioEvent>,
    pub stop_audio: bool,
}

impl HarnessResult {
    const fn none() -> Self {
        Self {
            needs_redraw: false,
            audio_event: None,
            stop_audio: false,
        }
    }
}

pub struct TestHarnessConfig {
    pub enabled: &'static [TestFunction],
    pub default: Option<TestFunction>,
    pub has_navigation: bool,
}

pub struct TestHarnessState {
    cfg: TestHarnessConfig,
    route: TestRoute,
    selected_idx: usize,
}

#[cfg(all(feature = "test-fw-screen-static", feature = "test-fw-audio-playback"))]
const ENABLED_TESTS: [TestFunction; 2] = [TestFunction::ScreenStatic, TestFunction::AudioPlayback];
#[cfg(all(
    feature = "test-fw-screen-static",
    not(feature = "test-fw-audio-playback")
))]
const ENABLED_TESTS: [TestFunction; 1] = [TestFunction::ScreenStatic];
#[cfg(all(
    not(feature = "test-fw-screen-static"),
    feature = "test-fw-audio-playback"
))]
const ENABLED_TESTS: [TestFunction; 1] = [TestFunction::AudioPlayback];
#[cfg(all(
    not(feature = "test-fw-screen-static"),
    not(feature = "test-fw-audio-playback")
))]
const ENABLED_TESTS: [TestFunction; 0] = [];

pub fn config_from_features() -> TestHarnessConfig {
    let enabled = &ENABLED_TESTS;
    let has_navigation = enabled.len() > 1;
    TestHarnessConfig {
        enabled,
        default: default_from_features(),
        has_navigation,
    }
}

impl TestHarnessState {
    pub fn new(cfg: TestHarnessConfig) -> Self {
        let mut selected_idx = 0usize;
        if let Some(default_fn) = cfg.default {
            selected_idx = index_of(cfg.enabled, default_fn).unwrap_or(0);
        }
        let route = if let Some(default_fn) = cfg.default {
            route_of(default_fn)
        } else if cfg.has_navigation {
            TestRoute::Navigation
        } else {
            route_of(cfg.enabled[0])
        };
        Self {
            cfg,
            route,
            selected_idx,
        }
    }

    pub fn route(&self) -> TestRoute {
        self.route
    }

    pub fn back_enabled(&self) -> bool {
        self.cfg.has_navigation
    }

    pub fn selected_function_ui(&self) -> TestFunctionUi {
        if self.cfg.enabled[self.selected_idx] == TestFunction::AudioPlayback {
            TestFunctionUi::AudioPlayback
        } else {
            TestFunctionUi::ScreenStatic
        }
    }

    pub fn default_function_ui(&self) -> Option<TestFunctionUi> {
        self.cfg.default.map(|v| match v {
            TestFunction::ScreenStatic => TestFunctionUi::ScreenStatic,
            TestFunction::AudioPlayback => TestFunctionUi::AudioPlayback,
        })
    }

    pub fn handle_input(&mut self, input: HarnessInputEvent) -> HarnessResult {
        match self.route {
            TestRoute::Navigation => self.handle_navigation_input(input),
            TestRoute::ScreenStatic => self.handle_screen_static_input(input),
            TestRoute::AudioPlayback => self.handle_audio_input(input),
        }
    }

    fn handle_navigation_input(&mut self, input: HarnessInputEvent) -> HarnessResult {
        let mut out = HarnessResult::none();
        match input {
            HarnessInputEvent::Up | HarnessInputEvent::Left => {
                if self.selected_idx == 0 {
                    self.selected_idx = self.cfg.enabled.len().saturating_sub(1);
                } else {
                    self.selected_idx -= 1;
                }
                out.needs_redraw = true;
                out.audio_event = Some(AudioEvent::ModeSwitch);
            }
            HarnessInputEvent::Down | HarnessInputEvent::Right => {
                self.selected_idx = (self.selected_idx + 1) % self.cfg.enabled.len();
                out.needs_redraw = true;
                out.audio_event = Some(AudioEvent::ModeSwitch);
            }
            HarnessInputEvent::Center => {
                self.route = route_of(self.cfg.enabled[self.selected_idx]);
                out.needs_redraw = true;
                out.audio_event = Some(AudioEvent::KeyInteraction);
            }
            HarnessInputEvent::Touch { x, y } => {
                if let Some(func) = test_navigation_hit_test(x, y) {
                    self.selected_idx = if func == TestFunctionUi::AudioPlayback {
                        index_of(self.cfg.enabled, TestFunction::AudioPlayback)
                            .unwrap_or(self.selected_idx)
                    } else {
                        index_of(self.cfg.enabled, TestFunction::ScreenStatic)
                            .unwrap_or(self.selected_idx)
                    };
                    self.route = route_of(self.cfg.enabled[self.selected_idx]);
                    out.needs_redraw = true;
                    out.audio_event = Some(AudioEvent::TouchInteraction);
                }
            }
        }
        out
    }

    fn handle_screen_static_input(&mut self, input: HarnessInputEvent) -> HarnessResult {
        let mut out = HarnessResult::none();
        match input {
            HarnessInputEvent::Left => {
                if self.cfg.has_navigation {
                    self.route = TestRoute::Navigation;
                    out.needs_redraw = true;
                    out.audio_event = Some(AudioEvent::ModeSwitch);
                }
            }
            HarnessInputEvent::Center => {
                out.audio_event = Some(AudioEvent::KeyInteraction);
            }
            HarnessInputEvent::Touch { x, y } => {
                if test_back_hit_test(x, y) {
                    if self.cfg.has_navigation {
                        self.route = TestRoute::Navigation;
                        out.needs_redraw = true;
                        out.audio_event = Some(AudioEvent::TouchInteraction);
                    }
                }
            }
            HarnessInputEvent::Up | HarnessInputEvent::Down | HarnessInputEvent::Right => {
                out.audio_event = Some(AudioEvent::KeyInteraction);
            }
        }
        out
    }

    fn handle_audio_input(&mut self, input: HarnessInputEvent) -> HarnessResult {
        let mut out = HarnessResult::none();
        match input {
            HarnessInputEvent::Left => {
                if self.cfg.has_navigation {
                    self.route = TestRoute::Navigation;
                    out.needs_redraw = true;
                    out.stop_audio = true;
                }
            }
            HarnessInputEvent::Up => out.audio_event = Some(AudioEvent::Warning),
            HarnessInputEvent::Down => out.audio_event = Some(AudioEvent::Error),
            HarnessInputEvent::Right => out.audio_event = Some(AudioEvent::ModeSwitch),
            HarnessInputEvent::Center => out.audio_event = Some(AudioEvent::KeyInteraction),
            HarnessInputEvent::Touch { x, y } => {
                if test_back_hit_test(x, y) {
                    if self.cfg.has_navigation {
                        self.route = TestRoute::Navigation;
                        out.needs_redraw = true;
                        out.stop_audio = true;
                    }
                } else if test_audio_play_hit_test(x, y) {
                    out.audio_event = Some(AudioEvent::Boot);
                } else if test_audio_stop_hit_test(x, y) {
                    out.stop_audio = true;
                    out.needs_redraw = true;
                }
            }
        }
        out
    }
}

fn route_of(func: TestFunction) -> TestRoute {
    match func {
        TestFunction::ScreenStatic => TestRoute::ScreenStatic,
        TestFunction::AudioPlayback => TestRoute::AudioPlayback,
    }
}

fn index_of(enabled: &[TestFunction], func: TestFunction) -> Option<usize> {
    let mut idx = 0usize;
    while idx < enabled.len() {
        if enabled[idx] == func {
            return Some(idx);
        }
        idx += 1;
    }
    None
}

#[cfg(feature = "test-fw-default-screen-static")]
fn default_from_features() -> Option<TestFunction> {
    Some(TestFunction::ScreenStatic)
}

#[cfg(all(
    not(feature = "test-fw-default-screen-static"),
    feature = "test-fw-default-audio-playback"
))]
fn default_from_features() -> Option<TestFunction> {
    Some(TestFunction::AudioPlayback)
}

#[cfg(all(
    not(feature = "test-fw-default-screen-static"),
    not(feature = "test-fw-default-audio-playback")
))]
fn default_from_features() -> Option<TestFunction> {
    None
}
