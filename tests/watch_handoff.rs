#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

#[test]
fn watcher_inherits_current_server_environment_after_handoff() {
    assert_watcher_adopts_environment("startup");
}

#[test]
fn refresh_repairs_a_watcher_when_plugin_enable_does_not_run_startup() {
    assert_watcher_adopts_environment("refresh");
}

fn assert_watcher_adopts_environment(entrypoint: &str) {
    let dir = tempfile::tempdir().unwrap();
    let old = dir.path().join("herdr-old");
    let current = dir.path().join("herdr-current");
    fs::write(
        &old,
        "#!/bin/sh\nprintf '%s\\n' 'protocol_mismatch' >&2\nexit 1\n",
    )
    .unwrap();
    fs::write(
        &current,
        r#"#!/bin/sh
[ "$HERDR_SOCKET_PATH" = "$TEST_CURRENT_SOCKET" ] || exit 2
printf '%s\n' "$*" >> "$TEST_LOG"
printf '%s\n' '{"result":{"agents":[]}}'
"#,
    )
    .unwrap();
    for path in [&old, &current] {
        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
    }
    let log = dir.path().join("calls");
    let socket = dir.path().join("current.sock");
    let command = || {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_herdr-agent-quota"));
        cmd.env("HERDR_PLUGIN_STATE_DIR", dir.path())
            .env("HERDR_PLUGIN_CONFIG_DIR", dir.path())
            .env("HERDR_AGENT_QUOTA_AGENT_ORDER", "default")
            .env("TEST_CURRENT_SOCKET", &socket)
            .env("TEST_LOG", &log)
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        cmd
    };
    assert!(command()
        .args([entrypoint, "--provider", "agy"])
        .env("HERDR_BIN_PATH", &current)
        .env("HERDR_SOCKET_PATH", &socket)
        .status()
        .unwrap()
        .success());
    // Startup may launch its own short-lived watcher; wait for its lease.
    let cache = herdr_agent_quota::cache::CacheStore::new(dir.path());
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        let expected = if entrypoint == "startup" { 2 } else { 1 };
        if fs::read_to_string(&log).unwrap().lines().count() >= expected
            && cache.try_lock_named("turn.lock").unwrap().is_some()
        {
            break;
        }
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(10));
    }
    let before = fs::read_to_string(&log).unwrap().lines().count();
    let mut child = command()
        .args(["watch", "--provider", "agy"])
        .env("HERDR_BIN_PATH", &old)
        .env("HERDR_SOCKET_PATH", dir.path().join("old.sock"))
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break Some(status);
        }
        if Instant::now() >= deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            break None;
        }
        thread::sleep(Duration::from_millis(10));
    };
    assert!(status.is_some_and(|status| status.success()),
        "watcher kept polling the old incompatible Herdr client instead of adopting startup's environment");
    assert!(fs::read_to_string(log).unwrap().lines().count() > before);
}
