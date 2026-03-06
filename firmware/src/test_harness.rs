use crate::front_panel_scene::{
    test_audio_list_hit_test, test_audio_list_scroll_hit_test, test_audio_stop_hit_test,
    test_back_hit_test, test_navigation_hit_test, TestFunctionUi, TEST_AUDIO_VISIBLE_ROWS,
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
    TouchDrag { x: u16, y: u16, dy: i16 },
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
    audio_selected_idx: usize,
    audio_list_top: usize,
}

const AUDIO_LIST_LEN: usize = 6;

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
            audio_selected_idx: 0,
            audio_list_top: 0,
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

    pub fn audio_selected_index(&self) -> usize {
        self.audio_selected_idx
    }

    pub fn audio_list_top(&self) -> usize {
        self.audio_list_top
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
            HarnessInputEvent::TouchDrag { .. } => {}
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
            HarnessInputEvent::TouchDrag { .. } => {}
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
            HarnessInputEvent::Up => {
                self.audio_move_selection(-1);
                out.needs_redraw = true;
                out.audio_event = Some(AudioEvent::KeyInteraction);
            }
            HarnessInputEvent::Down => {
                self.audio_move_selection(1);
                out.needs_redraw = true;
                out.audio_event = Some(AudioEvent::KeyInteraction);
            }
            HarnessInputEvent::Right => {
                out.stop_audio = true;
                out.needs_redraw = true;
            }
            HarnessInputEvent::Center => {
                out.audio_event = Some(audio_event_for_index(self.audio_selected_idx));
                out.needs_redraw = true;
            }
            HarnessInputEvent::Touch { x, y } => {
                if test_back_hit_test(x, y) {
                    if self.cfg.has_navigation {
                        self.route = TestRoute::Navigation;
                        out.needs_redraw = true;
                        out.stop_audio = true;
                    }
                } else if test_audio_stop_hit_test(x, y) {
                    out.stop_audio = true;
                    out.needs_redraw = true;
                } else if let Some(idx) = test_audio_list_hit_test(x, y, self.audio_list_top) {
                    self.audio_selected_idx = idx;
                    self.audio_ensure_visible();
                    out.audio_event = Some(audio_event_for_index(idx));
                    out.needs_redraw = true;
                }
            }
            HarnessInputEvent::TouchDrag { x, y, dy } => {
                if test_audio_list_scroll_hit_test(x, y) {
                    if self.audio_scroll_from_drag(dy) {
                        out.needs_redraw = true;
                    }
                }
            }
        }
        out
    }

    fn audio_move_selection(&mut self, delta: i32) {
        if delta < 0 {
            if self.audio_selected_idx == 0 {
                self.audio_selected_idx = AUDIO_LIST_LEN - 1;
            } else {
                self.audio_selected_idx -= 1;
            }
        } else if self.audio_selected_idx + 1 >= AUDIO_LIST_LEN {
            self.audio_selected_idx = 0;
        } else {
            self.audio_selected_idx += 1;
        }
        self.audio_ensure_visible();
    }

    fn audio_ensure_visible(&mut self) {
        let visible = TEST_AUDIO_VISIBLE_ROWS;
        if self.audio_selected_idx < self.audio_list_top {
            self.audio_list_top = self.audio_selected_idx;
            return;
        }
        let end = self.audio_list_top + visible;
        if self.audio_selected_idx >= end {
            self.audio_list_top = self.audio_selected_idx + 1 - visible;
        }
    }

    fn audio_scroll_from_drag(&mut self, dy: i16) -> bool {
        let max_top = AUDIO_LIST_LEN.saturating_sub(TEST_AUDIO_VISIBLE_ROWS);
        if dy >= 8 {
            if self.audio_list_top > 0 {
                self.audio_list_top -= 1;
                if self.audio_selected_idx < self.audio_list_top {
                    self.audio_selected_idx = self.audio_list_top;
                }
                return true;
            }
            return false;
        }
        if dy <= -8 {
            if self.audio_list_top < max_top {
                self.audio_list_top += 1;
                let max_visible = self.audio_list_top + TEST_AUDIO_VISIBLE_ROWS - 1;
                if self.audio_selected_idx > max_visible {
                    self.audio_selected_idx = max_visible;
                }
                return true;
            }
        }
        false
    }
}

fn audio_event_for_index(idx: usize) -> AudioEvent {
    match idx {
        0 => AudioEvent::Boot,
        1 => AudioEvent::TouchInteraction,
        2 => AudioEvent::KeyInteraction,
        3 => AudioEvent::ModeSwitch,
        4 => AudioEvent::Warning,
        _ => AudioEvent::Error,
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
