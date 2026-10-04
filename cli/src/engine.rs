//! Состояние набора и горячих клавиш без доступа к устройствам и часам ОС.

use std::{
    collections::HashSet,
    time::{Duration, Instant},
};

use crate::{
    config::Config,
    keys,
    layout::{self, Lang},
    state::{Buffer, Stroke},
};

#[derive(Debug)]
pub struct KeyEvent {
    pub device_id: u64,
    pub code: u16,
    pub value: i32,
}

#[derive(Debug)]
pub enum DeviceEvent {
    Resynced {
        device_id: u64,
        held_keys: Vec<u16>,
    },
    Key(KeyEvent),
    Click,
    Disconnected(u64),
    Session(Option<String>),
    LostEvents(u64),
    /// Активная раскладка; `None` - неизвестна или не пара EN/RU.
    Layout(Option<Lang>),
}

#[derive(Default)]
struct HeldKeys {
    keys: HashSet<(u64, u16)>,
}

impl HeldKeys {
    fn observe(&mut self, event: &DeviceEvent) {
        match event {
            DeviceEvent::Resynced {
                device_id,
                held_keys,
            } => {
                self.keys.retain(|(id, _)| id != device_id);
                self.keys
                    .extend(held_keys.iter().map(|code| (*device_id, *code)));
            }
            DeviceEvent::Key(event) => match event.value {
                0 => {
                    self.keys.remove(&(event.device_id, event.code));
                }
                1 => {
                    self.keys.insert((event.device_id, event.code));
                }
                _ => {}
            },
            DeviceEvent::Disconnected(device_id) => self.keys.retain(|(id, _)| id != device_id),
            _ => {}
        }
    }
    fn matches(&self, hotkey: &[u16]) -> bool {
        hotkey
            .iter()
            .all(|required| self.keys.iter().any(|(_, code)| code == required))
            && self.keys.iter().all(|(_, code)| hotkey.contains(code))
    }
    fn shift(&self) -> bool {
        self.keys.iter().any(|(_, code)| keys::is_shift(*code))
    }
    fn command(&self) -> bool {
        self.keys
            .iter()
            .any(|(_, code)| keys::is_command_modifier(*code))
    }
}

/// Начало исправления слова с `start`: короткие слова перед ним через один
/// пробел в той же чужой раскладке (`F jy` -> `А он`) исправляются вместе с ним.
fn short_words_before(phrase: &[Stroke], mut start: usize, shown: Lang) -> usize {
    while start >= 2 && phrase[start - 1].code == keys::KEY_SPACE {
        let end = start - 1;
        let begin = phrase[..end]
            .iter()
            .rposition(|stroke| keys::is_separator(stroke.code))
            .map_or(0, |index| index + 1);
        let letters: Vec<(u16, bool)> = phrase[begin..end]
            .iter()
            .map(|stroke| (stroke.code, stroke.shift))
            .collect();
        if letters.is_empty() || !layout::short_wrong(&letters, shown) {
            break;
        }
        start = begin;
    }
    start
}

pub struct PendingFix {
    pub strokes: Vec<Stroke>,
    pub phrase: bool,
    /// Найдено детектором на пробеле, а не по хоткею.
    pub auto: bool,
    trigger: u16,
    ready_at: Option<Instant>,
}

pub struct Engine {
    buffer: Buffer,
    held: HeldKeys,
    pending: Option<PendingFix>,
    pub paused: bool,
    session: Option<String>,
    last_input: Instant,
    unsynced: HashSet<u64>,
    layout: Option<Lang>,
}

impl Engine {
    pub fn new(cfg: &Config, now: Instant) -> Self {
        Self {
            buffer: Buffer::new(cfg.max_strokes),
            held: HeldKeys::default(),
            pending: None,
            paused: false,
            session: (!cfg.session_guard).then(|| "unguarded".to_string()),
            last_input: now,
            unsynced: HashSet::new(),
            layout: None,
        }
    }

    /// Коррекция переключила раскладку хоткеем: при двух раскладках - на другую.
    /// Сигнал KDE о той же смене после этого не сбрасывает буфер.
    pub fn switched(&mut self) {
        self.layout = self.layout.map(Lang::other);
    }

    /// Пробел после слова в чужой раскладке -> автоматическая коррекция слова с пробелом.
    fn check_last_word(&mut self, cfg: &Config) {
        let Some(shown) = self.layout.filter(|_| cfg.auto_switch) else {
            return;
        };
        let word = self.buffer.last_word();
        let letters: Vec<(u16, bool)> = word
            .iter()
            .take_while(|stroke| !keys::is_separator(stroke.code))
            .map(|stroke| (stroke.code, stroke.shift))
            .collect();
        // Ровно один пробел после слова: второй пробел слово уже не трогает.
        if word.len() == letters.len() + 1 && layout::wrong_layout(&letters, shown) {
            let phrase = self.buffer.phrase();
            let start = short_words_before(phrase, phrase.len() - word.len(), shown);
            self.pending = Some(PendingFix {
                strokes: phrase[start..].to_vec(),
                phrase: false,
                auto: true,
                trigger: keys::KEY_SPACE,
                ready_at: None,
            });
        }
    }

    fn observe_device_state(&mut self, event: &DeviceEvent) {
        self.held.observe(event);
        match event {
            DeviceEvent::LostEvents(id) => {
                self.unsynced.insert(*id);
            }
            DeviceEvent::Resynced { device_id, .. } | DeviceEvent::Disconnected(device_id) => {
                self.unsynced.remove(device_id);
            }
            _ => {}
        }
    }

    pub fn invalidate(&mut self) {
        self.buffer.clear();
        self.pending = None;
    }

    pub fn expire(&mut self, cfg: &Config, now: Instant) {
        if now.duration_since(self.last_input) >= Duration::from_millis(cfg.buffer_timeout_ms) {
            self.invalidate();
        }
    }

    /// Клавиша, пока коррекция ждёт отпускания всех клавиш.
    fn observe_pending(&mut self, event: &KeyEvent, now: Instant) {
        let Some(pending) = &mut self.pending else {
            return;
        };
        // Следующее слово, начатое до отпускания пробела, уже на экране в той же
        // чужой раскладке: перенабирается вместе с исправляемым.
        let typed =
            !self.held.command() && (keys::is_char(event.code) || keys::is_separator(event.code));
        if pending.auto && event.value == 1 && (typed || keys::is_shift(event.code)) {
            if typed {
                let shift = self.held.shift();
                self.buffer.push(event.code, shift);
                pending.strokes.push(Stroke {
                    code: event.code,
                    shift,
                });
            }
            pending.ready_at = None;
            self.last_input = now;
        } else if event.value == 1
            || (event.value == 2 && (pending.auto || event.code != pending.trigger))
        {
            self.invalidate();
        } else if self.held.keys.is_empty() {
            pending.ready_at = Some(now + Duration::from_millis(30));
        }
    }

    pub fn observe(&mut self, event: DeviceEvent, cfg: &Config, now: Instant) {
        self.expire(cfg, now);
        self.observe_device_state(&event);
        if let DeviceEvent::Session(session) = event {
            self.session = session;
            self.invalidate();
            return;
        }
        if let DeviceEvent::Layout(layout) = event {
            if self.layout != layout {
                self.layout = layout;
                self.invalidate();
            }
            return;
        }
        let DeviceEvent::Key(event) = event else {
            self.invalidate();
            return;
        };
        if event.value == 1
            && cfg.pause_hotkey.last() == Some(&event.code)
            && self.held.matches(&cfg.pause_hotkey)
        {
            self.paused = !self.paused;
            self.invalidate();
            return;
        }
        if self.paused || self.session.is_none() || !self.unsynced.is_empty() {
            self.invalidate();
            return;
        }
        if self.pending.is_some() {
            self.observe_pending(&event, now);
            return;
        }
        if event.value == 0 {
            return;
        }
        self.last_input = now;
        // Повтор на уровне evdev не гарантирует столько же символов в Wayland.
        if event.value != 1 {
            self.invalidate();
            return;
        }
        let phrase = if cfg.phrase_hotkey.last() == Some(&event.code)
            && self.held.matches(&cfg.phrase_hotkey)
        {
            Some(true)
        } else if cfg.hotkey.last() == Some(&event.code) && self.held.matches(&cfg.hotkey) {
            Some(false)
        } else {
            None
        };
        if let Some(phrase) = phrase {
            let strokes = if phrase {
                self.buffer.phrase()
            } else {
                self.buffer.last_word()
            };
            if !strokes.is_empty() {
                self.pending = Some(PendingFix {
                    strokes: strokes.to_vec(),
                    phrase,
                    auto: false,
                    trigger: event.code,
                    ready_at: None,
                });
            }
            return;
        }
        if self.held.matches(&cfg.layout_switch) {
            self.invalidate();
            return;
        }
        if keys::is_shift(event.code) || keys::is_command_modifier(event.code) {
            return;
        }
        if self.held.command() {
            self.invalidate();
        } else if event.code == keys::KEY_BACKSPACE {
            self.buffer.backspace();
        } else if event.code == keys::KEY_TAB || keys::is_phrase_end(event.code) {
            self.invalidate();
        } else if keys::is_char(event.code) || keys::is_separator(event.code) {
            self.buffer.push(event.code, self.held.shift());
            if event.code == keys::KEY_SPACE {
                self.check_last_word(cfg);
            }
        } else {
            self.invalidate();
        }
    }

    pub fn take_ready(&mut self, cfg: &Config, now: Instant) -> Option<PendingFix> {
        self.expire(cfg, now);
        if self
            .pending
            .as_ref()
            .is_some_and(|fix| fix.ready_at.is_some_and(|at| at <= now))
            && self.held.keys.is_empty()
        {
            self.pending.take()
        } else {
            None
        }
    }

    pub fn discard(&mut self, event: &DeviceEvent) {
        self.observe_device_state(event);
        // Раскладка - не ввод: её смена в старом поколении сессии всё равно действует.
        if let DeviceEvent::Layout(layout) = event {
            self.layout = *layout;
        }
        self.invalidate();
    }

    pub fn during_fix(&mut self, event: DeviceEvent, cfg: &Config, now: Instant) {
        self.invalidate();
        self.observe(event, cfg, now);
        self.invalidate();
    }
}

#[cfg(test)]
mod tests;
