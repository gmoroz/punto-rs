use super::*;

#[test]
fn dropped_events_are_ignored_until_report() {
    let mut stream = EventStream::default();
    assert_eq!(
        stream.observe(&InputEvent::new(EventType::SYNCHRONIZATION.0, 3, 0)),
        StreamAction::Lost
    );
    assert_eq!(
        stream.observe(&InputEvent::new(EventType::KEY.0, 30, 1)),
        StreamAction::Ignore
    );
    assert_eq!(
        stream.observe(&InputEvent::new(EventType::SYNCHRONIZATION.0, 0, 0)),
        StreamAction::Resync
    );
    assert_eq!(
        stream.observe(&InputEvent::new(EventType::KEY.0, 30, 0)),
        StreamAction::Key
    );
}

#[test]
fn key_message_keyboard_and_pointer_events_map_to_engine_input() {
    let key = |code, value| InputEvent::new(EventType::KEY.0, code, value);
    assert!(matches!(
        key_message(&key(30, 1), 7, DeviceKind::Keyboard),
        Some(DeviceEvent::Key(KeyEvent {
            device_id: 7,
            code: 30,
            value: 1
        }))
    ));
    for kind in [DeviceKind::Keyboard, DeviceKind::Pointer] {
        assert!(matches!(
            key_message(&key(keys::BTN_LEFT, 1), 7, kind),
            Some(DeviceEvent::Click)
        ));
        assert!(key_message(&key(keys::BTN_LEFT, 0), 7, kind).is_none());
    }
    assert!(key_message(&key(30, 1), 7, DeviceKind::Pointer).is_none());
}

#[test]
fn on_default_seat_missing_path_rejected_and_untagged_node_accepted() {
    assert!(!on_default_seat(Path::new("/nonexistent/punto-rs-input")));
    // Обычный файл даёт rdev 0: записи udev для c0:0 нет -> seat по умолчанию.
    assert!(on_default_seat(Path::new("Cargo.toml")));
}
