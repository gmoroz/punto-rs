use super::*;

struct Harness {
    engine: Engine,
    cfg: Config,
    now: Instant,
}
impl Harness {
    fn new() -> Self {
        let cfg = Config {
            session_guard: false,
            ..Config::default()
        };
        let now = Instant::now();
        Self {
            engine: Engine::new(&cfg, now),
            cfg,
            now,
        }
    }
    fn event(&mut self, event: DeviceEvent) {
        self.engine.observe(event, &self.cfg, self.now);
    }
    fn key(&mut self, device_id: u64, code: u16, value: i32) {
        self.event(DeviceEvent::Key(KeyEvent {
            device_id,
            code,
            value,
        }));
    }
    fn tap(&mut self, code: u16) {
        self.key(1, code, 1);
        self.key(1, code, 0);
    }
    fn ready(&mut self) -> Option<PendingFix> {
        self.now += Duration::from_millis(31);
        self.engine.take_ready(&self.cfg, self.now)
    }
    fn fix(&mut self) -> Option<PendingFix> {
        self.tap(keys::KEY_INSERT);
        self.ready()
    }
}

#[test]
fn word_and_phrase_preserve_shift_and_trailing_space() {
    let mut h = Harness::new();
    h.tap(16);
    h.tap(keys::KEY_SPACE);
    h.key(1, 42, 1);
    h.tap(17);
    h.key(1, 42, 0);
    h.tap(57);
    let fix = h.fix().unwrap();
    assert!(!fix.phrase);
    assert_eq!(
        fix.strokes,
        vec![
            Stroke {
                code: 17,
                shift: true
            },
            Stroke {
                code: 57,
                shift: false
            }
        ]
    );
    h.key(1, 125, 1);
    h.tap(110);
    h.key(1, 125, 0);
    let fix = h.ready().unwrap();
    assert!(fix.phrase);
    assert_eq!(fix.strokes.len(), 4);
}

#[test]
fn navigation_tab_shift_tab_and_click_forget_previous_field() {
    for code in [
        keys::KEY_TAB,
        keys::KEY_ENTER,
        keys::KEY_KPENTER,
        105,
        106,
        102,
        107,
        111,
    ] {
        for shift in [false, true] {
            let mut h = Harness::new();
            h.tap(30);
            if shift {
                h.key(1, 42, 1);
            }
            h.tap(code);
            if shift {
                h.key(1, 42, 0);
            }
            assert!(h.fix().is_none(), "code={code}, shift={shift}");
        }
    }
    let mut h = Harness::new();
    h.tap(30);
    h.event(DeviceEvent::Click);
    assert!(h.fix().is_none());
}

#[test]
fn shift_insert_does_not_correct() {
    let mut h = Harness::new();
    h.tap(30);
    h.key(1, 42, 1);
    h.tap(110);
    h.key(1, 42, 0);
    assert!(h.ready().is_none());
    assert!(h.fix().is_none());
}

#[test]
fn waits_for_release_and_cancels_on_new_input() {
    let mut h = Harness::new();
    h.tap(30);
    h.key(1, 110, 1);
    assert!(h.ready().is_none());
    h.key(1, 110, 0);
    assert!(h.engine.take_ready(&h.cfg, h.now).is_none());
    h.tap(48);
    assert!(h.ready().is_none());
    assert!(h.fix().is_none());
}

#[test]
fn same_modifier_on_two_keyboards_is_not_released_early() {
    let mut h = Harness::new();
    h.tap(30);
    h.key(1, 125, 1);
    h.key(2, 125, 1);
    h.tap(110);
    h.key(1, 125, 0);
    assert!(h.ready().is_none());
    h.key(2, 125, 0);
    assert!(h.ready().unwrap().phrase);
}

#[test]
fn loss_and_device_reconnect_cancel_pending_and_replace_held_state() {
    let mut h = Harness::new();
    h.tap(30);
    h.tap(110);
    h.event(DeviceEvent::LostEvents(1));
    assert!(h.ready().is_none());
    h.key(1, 29, 1);
    h.event(DeviceEvent::Resynced {
        device_id: 1,
        held_keys: vec![],
    });
    h.tap(30);
    assert!(h.fix().is_some());
    h.event(DeviceEvent::Disconnected(1));
    assert!(h.fix().is_none());
}

#[test]
fn no_correction_until_lost_device_is_resynchronized() {
    let mut h = Harness::new();
    h.event(DeviceEvent::LostEvents(2));
    h.tap(30);
    assert!(h.fix().is_none());
    h.event(DeviceEvent::Resynced {
        device_id: 2,
        held_keys: vec![],
    });
    h.tap(30);
    assert!(h.fix().is_some());
}

#[test]
fn pause_and_session_changes_discard_sensitive_history() {
    let mut h = Harness::new();
    h.tap(30);
    h.key(1, 125, 1);
    h.tap(119);
    h.key(1, 125, 0);
    assert!(h.engine.paused);
    h.tap(48);
    assert!(h.fix().is_none());
    h.key(1, 125, 1);
    h.tap(119);
    h.key(1, 125, 0);
    assert!(!h.engine.paused);
    assert!(h.fix().is_none());
    h.tap(30);
    h.event(DeviceEvent::Session(None));
    h.tap(48);
    assert!(h.fix().is_none());
    h.event(DeviceEvent::Session(Some("new-session".into())));
    assert!(h.fix().is_none());
    h.tap(30);
    assert!(h.fix().is_some());
}

#[test]
fn idle_and_repeat_invalidate_history() {
    let mut h = Harness::new();
    h.tap(30);
    h.now += Duration::from_millis(h.cfg.buffer_timeout_ms);
    assert!(h.fix().is_none());
    h.tap(30);
    h.key(1, 30, 2);
    assert!(h.fix().is_none());
}

#[test]
fn abort_does_not_retain_old_or_interleaved_text() {
    let mut h = Harness::new();
    h.tap(30);
    assert!(h.fix().is_some());
    h.engine.during_fix(
        DeviceEvent::Key(KeyEvent {
            device_id: 1,
            code: 48,
            value: 1,
        }),
        &h.cfg,
        h.now,
    );
    h.key(1, 48, 0);
    assert!(h.fix().is_none());
}

/// «привет», набранное в EN: ghbdtn.
const PRIVET_ON_EN: [u16; 6] = [34, 35, 48, 32, 20, 49];
/// «hello» в EN.
const HELLO: [u16; 5] = [35, 18, 38, 38, 24];

impl Harness {
    fn on_layout(layout: Option<Lang>) -> Self {
        let mut h = Self::new();
        h.event(DeviceEvent::Layout(layout));
        h
    }
    fn type_codes(&mut self, codes: &[u16]) {
        for &code in codes {
            self.tap(code);
        }
    }
}

#[test]
fn test_auto_wrong_word_then_space_schedules_word_with_space() {
    let mut h = Harness::on_layout(Some(Lang::En));
    h.type_codes(&PRIVET_ON_EN);
    h.tap(keys::KEY_SPACE);
    let fix = h.ready().unwrap();
    assert!(fix.auto);
    let codes: Vec<u16> = fix.strokes.iter().map(|stroke| stroke.code).collect();
    assert_eq!(codes, [&PRIVET_ON_EN[..], &[keys::KEY_SPACE]].concat());
}

#[test]
fn test_auto_single_letter_alone_kept_before_fixed_word_joins_fix() {
    // «f jy» в EN = «а он»; «a jy» - английское «a» остаётся.
    const F: u16 = 33;
    const A: u16 = 30;
    const ON_ON_EN: [u16; 3] = [36, 21, keys::KEY_SPACE];
    let mut h = Harness::on_layout(Some(Lang::En));
    h.type_codes(&[F, keys::KEY_SPACE]);
    assert!(h.ready().is_none());
    h.type_codes(&ON_ON_EN);
    let codes: Vec<u16> = h.ready().unwrap().strokes.iter().map(|s| s.code).collect();
    assert_eq!(codes, [&[F, keys::KEY_SPACE][..], &ON_ON_EN].concat());
    let mut h = Harness::on_layout(Some(Lang::En));
    h.type_codes(&[A, keys::KEY_SPACE]);
    h.type_codes(&ON_ON_EN);
    assert_eq!(h.ready().unwrap().strokes.len(), ON_ON_EN.len());
}

#[test]
fn test_auto_exception_short_word_before_fix_not_joined() {
    // «ру пше» в RU: «пше» -> git исправляется, «ру» из исключений не цепляется.
    const RU_ON_RU: [u16; 2] = [35, 18];
    const GIT_ON_RU: [u16; 3] = [34, 23, 20];
    let mut h = Harness::on_layout(Some(Lang::Ru));
    h.type_codes(&RU_ON_RU);
    h.tap(keys::KEY_SPACE);
    assert!(h.ready().is_none());
    h.type_codes(&GIT_ON_RU);
    h.tap(keys::KEY_SPACE);
    let codes: Vec<u16> = h.ready().unwrap().strokes.iter().map(|s| s.code).collect();
    assert_eq!(codes, [&GIT_ON_RU[..], &[keys::KEY_SPACE]].concat());
}

#[test]
fn test_auto_right_word_unknown_layout_or_disabled_not_scheduled() {
    let mut h = Harness::on_layout(Some(Lang::En));
    h.type_codes(&HELLO);
    h.tap(keys::KEY_SPACE);
    assert!(h.ready().is_none());
    let mut h = Harness::on_layout(None);
    h.type_codes(&PRIVET_ON_EN);
    h.tap(keys::KEY_SPACE);
    assert!(h.ready().is_none());
    let mut h = Harness::on_layout(Some(Lang::En));
    h.cfg.auto_switch = false;
    h.type_codes(&PRIVET_ON_EN);
    h.tap(keys::KEY_SPACE);
    assert!(h.ready().is_none());
}

#[test]
fn test_auto_next_key_before_space_release_extends_fix() {
    let mut h = Harness::on_layout(Some(Lang::En));
    h.type_codes(&PRIVET_ON_EN);
    h.key(1, keys::KEY_SPACE, 1);
    h.key(1, 19, 1);
    h.key(1, keys::KEY_SPACE, 0);
    assert!(h.ready().is_none());
    h.key(1, 19, 0);
    let fix = h.ready().unwrap();
    assert_eq!(fix.strokes.len(), PRIVET_ON_EN.len() + 2);
    assert_eq!(fix.strokes.last().map(|stroke| stroke.code), Some(19));
}

#[test]
fn test_auto_backspace_or_held_space_cancels_fix() {
    let mut h = Harness::on_layout(Some(Lang::En));
    h.type_codes(&PRIVET_ON_EN);
    h.key(1, keys::KEY_SPACE, 1);
    h.key(1, keys::KEY_SPACE, 2);
    h.key(1, keys::KEY_SPACE, 0);
    assert!(h.ready().is_none());
    let mut h = Harness::on_layout(Some(Lang::En));
    h.type_codes(&PRIVET_ON_EN);
    h.key(1, keys::KEY_SPACE, 1);
    h.tap(keys::KEY_BACKSPACE);
    h.key(1, keys::KEY_SPACE, 0);
    assert!(h.ready().is_none());
}

#[test]
fn test_layout_event_new_layout_forgets_history_same_keeps_it() {
    let mut h = Harness::on_layout(Some(Lang::En));
    h.tap(30);
    h.event(DeviceEvent::Layout(Some(Lang::En)));
    assert!(h.fix().is_some());
    h.tap(30);
    h.event(DeviceEvent::Layout(Some(Lang::Ru)));
    assert!(h.fix().is_none());
}

#[test]
fn test_switched_toggles_layout_so_own_signal_keeps_history() {
    let mut h = Harness::on_layout(Some(Lang::En));
    h.type_codes(&PRIVET_ON_EN);
    h.tap(keys::KEY_SPACE);
    assert!(h.ready().is_some());
    h.engine.switched();
    h.event(DeviceEvent::Layout(Some(Lang::Ru)));
    let undo = h.fix().unwrap();
    assert!(!undo.auto);
    assert_eq!(undo.strokes.len(), PRIVET_ON_EN.len() + 1);
}
