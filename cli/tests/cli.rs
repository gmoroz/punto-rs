use std::process::Command;

#[test]
fn config_check_is_read_only_and_needs_no_input_devices() {
    let output = Command::new(env!("CARGO_BIN_EXE_punto-rs"))
        .args(["--check-config", "--config", "config/punto-rs.conf"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("конфиг корректен"));
}

#[test]
fn explicit_missing_config_fails_before_opening_devices() {
    let output = Command::new(env!("CARGO_BIN_EXE_punto-rs"))
        .args(["--config", "/nonexistent/punto-rs.conf"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("не прочитан"));
    assert!(!stderr.contains("uinput"));
    assert!(!stderr.contains("блокировка экземпляра"));
}

#[test]
fn invalid_arguments_exit_with_usage_error() {
    for (args, message) in [
        (&["--bogus"][..], "неизвестный аргумент: --bogus"),
        (&["--config"][..], "--config требует путь к файлу"),
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_punto-rs"))
            .args(args)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2), "{args:?}");
        assert!(String::from_utf8_lossy(&output.stderr).contains(message));
    }
}

#[test]
fn default_config_is_read_from_xdg_config_home_then_home() {
    let root = std::env::temp_dir().join(format!("punto-cli-config-{}", std::process::id()));
    let config_dir = root.join(".config/punto-rs");
    std::fs::create_dir_all(&config_dir).unwrap();
    std::fs::write(config_dir.join("config.conf"), "hotkey=pause\n").unwrap();
    for (var, value) in [
        ("HOME", root.clone()),
        ("XDG_CONFIG_HOME", root.join(".config")),
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_punto-rs"))
            .arg("--check-config")
            .env_remove("HOME")
            .env_remove("XDG_CONFIG_HOME")
            .env(var, &value)
            .output()
            .unwrap();
        assert!(output.status.success(), "{var}: {output:?}");
    }
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn missing_home_or_runtime_dir_fails_before_opening_devices() {
    let output = Command::new(env!("CARGO_BIN_EXE_punto-rs"))
        .arg("--check-config")
        .env_remove("HOME")
        .env_remove("XDG_CONFIG_HOME")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("не заданы XDG_CONFIG_HOME и HOME"));
    let output = Command::new(env!("CARGO_BIN_EXE_punto-rs"))
        .args(["--config", "config/punto-rs.conf"])
        .env_remove("XDG_RUNTIME_DIR")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("XDG_RUNTIME_DIR не задан"));
    assert!(!stderr.contains("uinput"));
}

#[test]
fn list_devices_succeeds_without_opening_uinput() {
    let output = Command::new(env!("CARGO_BIN_EXE_punto-rs"))
        .arg("--list-devices")
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(!String::from_utf8_lossy(&output.stderr).contains("uinput"));
}

#[test]
fn help_and_version_work_without_configuration_or_privileges() {
    for option in ["--help", "--version"] {
        let output = Command::new(env!("CARGO_BIN_EXE_punto-rs"))
            .arg(option)
            .output()
            .unwrap();
        assert!(output.status.success());
        assert!(String::from_utf8_lossy(&output.stdout).contains("punto-rs"));
    }
}
