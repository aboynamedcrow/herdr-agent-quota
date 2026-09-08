use herdr_agent_quota::cache::CacheStore;
use serde_json::{json, Value};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;
use tempfile::tempdir;

const A: &str = "11111111-1111-4111-8111-111111111111";
const B: &str = "22222222-2222-4222-8222-222222222222";
const C: &str = "33333333-3333-4333-8333-333333333333";

fn executable(path: &Path, body: &str) {
    fs::write(path, body).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}
fn account(root: &Path, name: &str, id: &str, sessions: &[&str]) {
    let home = root.join(name);
    fs::create_dir_all(home.join("sessions")).unwrap();
    fs::write(
        home.join("auth.json"),
        json!({"tokens":{"account_id":id}}).to_string(),
    )
    .unwrap();
    for session in sessions {
        fs::write(
            home.join(format!("sessions/rollout-2099-01-01-{session}.jsonl")),
            json!({"type":"session_meta","payload":{"id":session}}).to_string(),
        )
        .unwrap();
    }
}
fn setup(root: &Path) {
    account(root, "a", "account-a", &[A, C]);
    account(root, "b", "account-b", &[B]);
    fs::create_dir_all(root.join("config")).unwrap();
    fs::write(
        root.join("config/codex-homes"),
        json!([root.join("a"), root.join("b")]).to_string(),
    )
    .unwrap();
    let panes = json!({"agents":[
        {"agent":"codex","pane_id":"w1:p1","agent_session":{"kind":"id","value":A}},
        {"agent":"codex","pane_id":"w1:p2","agent_session":{"kind":"id","value":B}},
        {"agent":"codex","pane_id":"w1:p3","agent_session":{"kind":"id","value":C}}
    ]});
    fs::write(root.join("inventory.json"), panes.to_string()).unwrap();
    executable(
        &root.join("herdr"),
        r#"#!/usr/bin/env python3
import json, os, sys
root = os.environ['TEST_ROOT']
args = sys.argv[1:]
with open(root + '/herdr.log', 'a') as f: f.write(json.dumps(args) + '\n')
if args == ['agent', 'list']: print(open(root + '/inventory.json').read())
elif args == ['pane', 'current']: print(json.dumps({'result':{'pane':{'pane_id':'w1:p1','agent':'codex'}}}))
"#,
    );
    executable(
        &root.join("codex"),
        r#"#!/usr/bin/env python3
import json, os, sys
root = os.environ['TEST_ROOT']
home = os.environ['CODEX_HOME']
account = json.load(open(home + '/auth.json'))['tokens']['account_id']
if os.path.basename(home) != 'wrong-home':
    assert 'OPENAI_API_KEY' not in os.environ
    assert 'CODEX_AUTH_FILE' not in os.environ
with open(root + '/codex.log', 'a') as f: f.write(account + '\n')
for line in sys.stdin:
    req = json.loads(line)
    if 'id' not in req: continue
    method = req['method']
    if method == 'initialize': result = {}
    elif method == 'account/read':
        identity = open(root + '/rpc-account').read().strip() if os.path.exists(root + '/rpc-account') else account
        result = {'account':{'type':'chatgpt', 'accountId':identity}}
    elif method == 'account/rateLimits/read':
        if os.path.exists(root + '/fail-fetch'): sys.exit(1)
        if os.path.exists(root + '/switch-during-fetch'):
            with open(home + '/auth.json', 'w') as f: json.dump({'tokens':{'account_id':'account-switched'}}, f)
        if os.path.exists(root + '/same-org-switch'):
            with open(home + '/auth.json', 'w') as f: json.dump({'tokens':{'account_id':account}, 'synthetic_seat':'new'}, f)
        used = 12 if os.path.basename(home) == 'a' else 73
        if os.path.exists(root + '/used-percent.json'):
            used = json.load(open(root + '/used-percent.json'))[os.path.basename(home)]
        result = {'rateLimits':{'primary':{'windowDurationMins':300,'usedPercent':used,'resetsAt':4000000000},'secondary':{'windowDurationMins':10080,'usedPercent':used,'resetsAt':4000500000}}}
        if os.path.exists(root + '/omit-week'): result['rateLimits']['secondary'] = None
    elif method == 'thread/list' and os.path.basename(home) == 'wrong-home': result = {'data':[]}
    else: raise Exception('unexpected RPC: ' + method)
    print(json.dumps({'id':req['id'],'result':result}), flush=True)
"#,
    );
}
fn refresh(root: &Path) {
    invoke(root, &["refresh", "--provider", "codex"]);
}

fn invoke(root: &Path, args: &[&str]) {
    invoke_for_pane(root, args, "w1:p1");
}

fn invoke_for_pane(root: &Path, args: &[&str], pane: &str) {
    let previous_log_lines = fs::read_to_string(root.join("herdr.log"))
        .unwrap_or_default()
        .lines()
        .count();
    let output = Command::new(env!("CARGO_BIN_EXE_herdr-agent-quota"))
        .args(args)
        .env("TEST_ROOT", root)
        .env("HERDR_PLUGIN_STATE_DIR", root.join("cache"))
        .env("HERDR_PLUGIN_CONFIG_DIR", root.join("config"))
        .env("HERDR_BIN_PATH", root.join("herdr"))
        .env("CODEX_BIN_PATH", root.join("codex"))
        .env("CODEX_HOME", root.join("wrong-home"))
        .env("CODEX_AUTH_FILE", root.join("wrong-auth"))
        .env("OPENAI_API_KEY", "synthetic-test-key")
        .env("PI_CODING_AGENT_DIR", root.join("pi"))
        .env("PI_CODING_AGENT_SESSION_DIR", root.join("pi/sessions"))
        .env(
            "HERDR_PLUGIN_EVENT_JSON",
            json!({"data":{"pane_id":pane,"agent":"codex","status":"idle"}}).to_string(),
        )
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    if args == ["event"] {
        let calls: Vec<Vec<String>> = fs::read_to_string(root.join("herdr.log"))
            .unwrap()
            .lines()
            .skip(previous_log_lines)
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        let reads: Vec<_> = calls
            .iter()
            .filter(|args| args.starts_with(&["pane".into(), "read".into()]))
            .collect();
        assert_eq!(reads.len(), 1);
        assert_eq!(
            reads[0],
            &["pane", "read", pane, "--source", "visible", "--format", "text"]
        );
        let reports: Vec<_> = calls
            .iter()
            .filter(|args| args.starts_with(&["pane".into(), "report-metadata".into()]))
            .collect();
        assert!(reports.len() <= 1);
        assert!(reports.iter().all(|args| args[2] == pane));
    }
}

fn notification_count(root: &Path) -> usize {
    fs::read_to_string(root.join("herdr.log"))
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str::<Vec<String>>(line).unwrap())
        .filter(|args| args.starts_with(&["notification".into(), "show".into()]))
        .inspect(|args| assert_eq!(args[2], "Codex quota is low"))
        .count()
}

fn expire_refresh_markers(root: &Path) {
    for entry in fs::read_dir(root.join("cache")).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_some_and(|ext| ext == "refresh") {
            fs::write(path, "0").unwrap();
        }
    }
}

#[test]
fn followup_alerts_preserve_each_account_until_real_recovery() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    setup(root);
    CacheStore::new(root.join("cache"))
        .set_low_quota_alert(herdr_agent_quota::cli::LowQuotaAlert::parse("10").unwrap())
        .unwrap();
    fs::write(
        root.join("used-percent.json"),
        json!({"a":92,"b":20}).to_string(),
    )
    .unwrap();
    invoke_for_pane(root, &["event"], "w1:p1");
    assert_eq!(notification_count(root), 1);
    invoke_for_pane(root, &["event"], "w1:p2");
    invoke_for_pane(root, &["event"], "w1:p1");
    assert_eq!(
        notification_count(root),
        1,
        "healthy B must not rearm low A"
    );
    // Rotation changes the cache generation, but not the warning identity.
    let rotated = json!({"tokens":{"account_id":"account-a"},"synthetic_rotation":1}).to_string();
    fs::write(root.join("a/auth.json"), &rotated).unwrap();
    invoke_for_pane(root, &["event"], "w1:p1");
    assert_eq!(notification_count(root), 1);
    let remembered = CacheStore::new(root.join("cache")).low_quota_alerted();
    fs::remove_file(root.join("a/auth.json")).unwrap();
    invoke_for_pane(root, &["event"], "w1:p1");
    assert_eq!(
        CacheStore::new(root.join("cache")).low_quota_alerted(),
        remembered
    );
    let inventory = fs::read_to_string(root.join("inventory.json")).unwrap();
    let mut only_b: Value = serde_json::from_str(&inventory).unwrap();
    only_b["agents"]
        .as_array_mut()
        .unwrap()
        .retain(|pane| pane["pane_id"] == "w1:p2");
    fs::write(root.join("inventory.json"), only_b.to_string()).unwrap();
    refresh(root);
    assert_eq!(
        CacheStore::new(root.join("cache")).low_quota_alerted(),
        remembered
    );
    fs::write(root.join("inventory.json"), inventory).unwrap();
    fs::write(root.join("a/auth.json"), rotated).unwrap();
    invoke_for_pane(root, &["event"], "w1:p1");
    assert_eq!(notification_count(root), 1);
    fs::write(
        root.join("used-percent.json"),
        json!({"a":40,"b":20}).to_string(),
    )
    .unwrap();
    expire_refresh_markers(root);
    invoke_for_pane(root, &["event"], "w1:p1");
    assert!(CacheStore::new(root.join("cache"))
        .low_quota_alerted()
        .is_empty());
    fs::write(
        root.join("used-percent.json"),
        json!({"a":92,"b":20}).to_string(),
    )
    .unwrap();
    expire_refresh_markers(root);
    invoke_for_pane(root, &["event"], "w1:p1");
    assert_eq!(notification_count(root), 2, "only actual recovery rearms A");
}

#[test]
fn followup_repeated_rotations_keep_file_count_bounded_and_values_fresh() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    setup(root);
    refresh(root);
    let paths = || {
        let mut paths: Vec<_> = fs::read_dir(root.join("cache"))
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        paths.sort();
        paths
    };
    let original_paths = paths();
    assert_eq!(original_paths.len(), 6);
    for rotation in 1..=8 {
        let id = if rotation < 5 {
            "account-a"
        } else {
            "account-new"
        };
        fs::write(
            root.join("a/auth.json"),
            json!({"tokens":{"account_id":id},"synthetic_rotation":rotation}).to_string(),
        )
        .unwrap();
        fs::write(
            root.join("used-percent.json"),
            json!({"a":rotation * 10,"b":73}).to_string(),
        )
        .unwrap();
        fs::write(root.join("herdr.log"), "").unwrap();
        refresh(root);
        assert_eq!(
            paths(),
            original_paths,
            "rotation {rotation} orphaned files"
        );
        let reports = writes(root);
        let a = reports
            .iter()
            .find(|args| args[2] == "w1:p1")
            .unwrap()
            .join(" ");
        assert!(a.contains(&format!("{}%", 100 - rotation * 10)), "{a}");
        let b = reports
            .iter()
            .find(|args| args[2] == "w1:p2")
            .unwrap()
            .join(" ");
        assert!(b.contains("27%"), "{b}");
        assert_eq!(
            fs::read_to_string(root.join("codex.log"))
                .unwrap()
                .lines()
                .count(),
            2 + rotation
        );
    }
}

#[test]
fn followup_symlink_auth_is_unavailable_even_inside_the_home() {
    for inside in [true, false] {
        let dir = tempdir().unwrap();
        let root = dir.path();
        setup(root);
        refresh(root);
        let target = root.join(if inside {
            "a/auth-target.json"
        } else {
            "auth-target.json"
        });
        fs::rename(root.join("a/auth.json"), &target).unwrap();
        std::os::unix::fs::symlink(&target, root.join("a/auth.json")).unwrap();
        fs::write(root.join("herdr.log"), "").unwrap();
        refresh(root);
        let reports = writes(root);
        let a = reports
            .iter()
            .find(|args| args[2] == "w1:p1")
            .unwrap()
            .join(" ");
        assert!(a.contains("identity unavailable"), "{a}");
        assert!(!a.contains("88%"), "{a}");
        assert_eq!(
            fs::read_to_string(root.join("codex.log"))
                .unwrap()
                .lines()
                .count(),
            2
        );
    }
    assert!(include_str!("../README.md").contains("symlinked `auth.json`"));
}

#[test]
fn followup_generation_changes_cannot_merge_old_windows_or_last_good_quota() {
    use herdr_agent_quota::model::{ProviderSnapshot, WindowKind};
    let dir = tempdir().unwrap();
    let root = dir.path();
    setup(root);
    refresh(root);
    let a_snapshot = fs::read_dir(root.join("cache"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
        .find(|path| {
            serde_json::from_slice::<Value>(&fs::read(path).unwrap()).unwrap()["account_id"]
                == "account-a"
        })
        .unwrap();
    fs::write(
        root.join("a/auth.json"),
        json!({"tokens":{"account_id":"account-a"},"synthetic_rotation":1}).to_string(),
    )
    .unwrap();
    fs::write(root.join("omit-week"), "").unwrap();
    refresh(root);
    let snapshot: ProviderSnapshot =
        serde_json::from_slice(&fs::read(&a_snapshot).unwrap()).unwrap();
    assert!(snapshot.credential_generation.is_some());
    assert!(snapshot.window(WindowKind::FiveHour).is_some());
    assert!(
        snapshot.window(WindowKind::Weekly).is_none(),
        "a previous generation cannot fill the omitted window"
    );
    for missing_stamp in [false, true] {
        if missing_stamp {
            let mut snapshot: Value =
                serde_json::from_slice(&fs::read(&a_snapshot).unwrap()).unwrap();
            snapshot
                .as_object_mut()
                .unwrap()
                .remove("credential_generation");
            fs::write(&a_snapshot, snapshot.to_string()).unwrap();
            expire_refresh_markers(root);
        } else {
            fs::write(
                root.join("a/auth.json"),
                json!({"tokens":{"account_id":"account-a"},"synthetic_rotation":2}).to_string(),
            )
            .unwrap();
        }
        fs::write(root.join("fail-fetch"), "").unwrap();
        fs::write(root.join("herdr.log"), "").unwrap();
        refresh(root);
        let reports = writes(root);
        let a = reports
            .iter()
            .find(|args| args[2] == "w1:p1")
            .unwrap()
            .join(" ");
        assert!(a.contains("N/A"), "{a}");
        assert!(!a.contains("88%"), "{a}");
        let calls = fs::read_to_string(root.join("codex.log")).unwrap();
        refresh(root);
        assert_eq!(
            fs::read_to_string(root.join("codex.log")).unwrap(),
            calls,
            "failed attempts still debounce within their generation"
        );
    }
}
fn writes(root: &Path) -> Vec<Vec<String>> {
    fs::read_to_string(root.join("herdr.log"))
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str::<Vec<String>>(line).unwrap())
        .filter(|args| args.starts_with(&["pane".into(), "report-metadata".into()]))
        .collect()
}
#[test]
fn distinct_accounts_collect_and_publish_separately_and_same_account_deduplicates() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    setup(root);
    refresh(root);
    let calls = fs::read_to_string(root.join("codex.log")).unwrap();
    assert_eq!(calls.lines().filter(|id| *id == "account-a").count(), 1);
    assert_eq!(calls.lines().filter(|id| *id == "account-b").count(), 1);
    let reports = writes(root);
    assert_eq!(reports.len(), 3);
    for (pane, percent) in [("w1:p1", "88%"), ("w1:p2", "27%"), ("w1:p3", "88%")] {
        let report = reports
            .iter()
            .find(|args| args[2] == pane)
            .unwrap()
            .join(" ");
        assert!(report.contains(percent), "{report}");
    }
    assert!(!fs::read_to_string(root.join("herdr.log"))
        .unwrap()
        .contains("\"read\""));
    // Mirror published metadata into the inventory: a cached refresh is a no-op.
    let mut inventory: Value =
        serde_json::from_str(&fs::read_to_string(root.join("inventory.json")).unwrap()).unwrap();
    for pane in inventory["agents"].as_array_mut().unwrap() {
        let report = reports
            .iter()
            .find(|args| args[2] == pane["pane_id"])
            .unwrap();
        let mut tokens = serde_json::Map::new();
        for pair in report.windows(2).filter(|pair| pair[0] == "--token") {
            let (key, value) = pair[1].split_once('=').unwrap();
            tokens.insert(key.into(), value.into());
        }
        pane["tokens"] = tokens.into();
    }
    fs::write(root.join("inventory.json"), inventory.to_string()).unwrap();
    refresh(root);
    assert_eq!(writes(root).len(), 3);
    assert_eq!(fs::read_to_string(root.join("codex.log")).unwrap(), calls);
    assert!(CacheStore::new(root.join("cache"))
        .load(herdr_agent_quota::model::Provider::Codex)
        .unwrap()
        .is_none());
}
#[test]
fn ambiguous_and_missing_sessions_never_use_an_account_cache() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    setup(root);
    refresh(root);
    account(root, "b", "account-b", &[A]);
    fs::remove_file(root.join(format!("a/sessions/rollout-2099-01-01-{C}.jsonl"))).unwrap();
    fs::write(root.join("herdr.log"), "").unwrap();
    refresh(root);
    for pane in ["w1:p1", "w1:p3"] {
        let reports = writes(root);
        let report = reports
            .iter()
            .find(|args| args[2] == pane)
            .unwrap()
            .join(" ");
        assert!(report.contains("N/A"), "{report}");
        assert!(!report.contains("signed-in account changed"), "{report}");
        assert!(!report.contains("88%"), "{report}");
    }
}

#[test]
fn homes_with_a_shared_subscription_id_do_not_share_seat_quota() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    setup(root);
    // Team subscriptions can expose the same account id to distinct seats.
    account(root, "b", "account-a", &[B]);
    refresh(root);
    let reports = writes(root);
    let b = reports
        .iter()
        .find(|args| args[2] == "w1:p2")
        .unwrap()
        .join(" ");
    assert!(b.contains("27%"), "{b}");
    assert_eq!(
        fs::read_to_string(root.join("codex.log"))
            .unwrap()
            .lines()
            .count(),
        2
    );
}

#[test]
fn changed_auth_fetches_new_scope_and_missing_auth_clears_old_numbers() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    setup(root);
    refresh(root);
    let calls = fs::read_to_string(root.join("codex.log")).unwrap();
    account(root, "a", "account-new", &[A, C]);
    fs::write(root.join("fail-fetch"), "").unwrap();
    fs::write(root.join("herdr.log"), "").unwrap();
    refresh(root);
    let after = fs::read_to_string(root.join("codex.log")).unwrap();
    assert_eq!(after.lines().count(), calls.lines().count() + 1);
    for report in writes(root).iter().filter(|args| args[2] != "w1:p2") {
        assert!(report.join(" ").contains("N/A"));
        assert!(!report.join(" ").contains("88%"));
    }
    fs::remove_file(root.join("b/auth.json")).unwrap();
    fs::write(root.join("herdr.log"), "").unwrap();
    refresh(root);
    let reports = writes(root);
    let report = reports
        .iter()
        .find(|args| args[2] == "w1:p2")
        .unwrap()
        .join(" ");
    assert!(report.contains("identity unavailable"), "{report}");
    assert!(!report.contains("signed-in account changed"));
    assert_eq!(fs::read_to_string(root.join("codex.log")).unwrap(), after);
}

#[test]
fn rpc_mismatch_or_auth_switch_during_collection_never_publishes_quota() {
    for marker in ["rpc-account", "switch-during-fetch", "same-org-switch"] {
        let dir = tempdir().unwrap();
        let root = dir.path();
        setup(root);
        fs::write(root.join(marker), "different-account").unwrap();
        refresh(root);
        let reports = writes(root);
        assert_eq!(reports.len(), 3);
        for report in reports {
            assert!(report.join(" ").contains("N/A"), "{report:?}");
            assert!(!report.join(" ").contains("88%"));
            assert!(!report.join(" ").contains("27%"));
        }
        assert!(!fs::read_dir(root.join("cache")).unwrap().any(|file| file
            .unwrap()
            .path()
            .extension()
            .is_some_and(|ext| ext == "json")));
    }
}

#[test]
fn default_single_home_needs_no_allowlist_and_reads_only_its_exact_session() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    setup(root);
    fs::remove_file(root.join("config/codex-homes")).unwrap();
    fs::rename(root.join("a"), root.join("wrong-home")).unwrap();
    refresh(root);
    assert_eq!(
        fs::read_to_string(root.join("codex.log")).unwrap(),
        "account-a\n"
    );
    let reports = writes(root);
    let a = reports
        .iter()
        .find(|args| args[2] == "w1:p1")
        .unwrap()
        .join(" ");
    let b = reports
        .iter()
        .find(|args| args[2] == "w1:p2")
        .unwrap()
        .join(" ");
    assert!(a.contains("27%"), "{a}");
    assert!(b.contains("N/A"), "{b}");
}

#[test]
fn native_entry_points_obey_pane_read_and_publish_limits() {
    for entry in ["event", "focus", "startup", "watch"] {
        let dir = tempdir().unwrap();
        let root = dir.path();
        setup(root);
        let mut inventory: Value =
            serde_json::from_str(&fs::read_to_string(root.join("inventory.json")).unwrap())
                .unwrap();
        inventory["agents"][0]["tokens"] = json!({"quota_topic":"Keep the existing topic"});
        fs::write(root.join("inventory.json"), inventory.to_string()).unwrap();
        if entry == "startup" || entry == "watch" {
            invoke(root, &[entry, "--provider", "codex"]);
        } else {
            invoke(root, &[entry]);
        }
        let log = fs::read_to_string(root.join("herdr.log")).unwrap();
        let calls: Vec<Vec<String>> = log
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        let reads: Vec<_> = calls
            .iter()
            .filter(|args| args.starts_with(&["pane".into(), "read".into()]))
            .collect();
        if entry == "event" {
            assert_eq!(reads.len(), 1);
            assert_eq!(
                reads[0],
                &["pane", "read", "w1:p1", "--source", "visible", "--format", "text"]
            );
        } else {
            assert!(reads.is_empty(), "{entry}: {reads:?}");
        }
        assert!(!log.contains("recent"));
        let reports = writes(root);
        let expected = match entry {
            "startup" => 3,
            "watch" => 0,
            _ => 1,
        };
        assert_eq!(reports.len(), expected, "{entry}: {reports:?}");
        if entry == "watch" {
            assert!(!root.join("codex.log").exists());
        } else {
            assert!(reports
                .iter()
                .find(|report| report[2] == "w1:p1")
                .unwrap()
                .join(" ")
                .contains("Keep the existing topic"));
        }
        if matches!(entry, "event" | "focus") {
            assert_eq!(
                fs::read_to_string(root.join("codex.log")).unwrap(),
                "account-a\n"
            );
        }
    }
}

#[test]
fn scoped_cache_preserves_omitted_windows_and_failed_fetches_for_only_that_account() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    setup(root);
    refresh(root);
    for marker in ["omit-week", "fail-fetch"] {
        fs::write(root.join(marker), "").unwrap();
        for entry in fs::read_dir(root.join("cache")).unwrap() {
            let path = entry.unwrap().path();
            if path
                .extension()
                .is_some_and(|extension| extension == "refresh")
            {
                fs::write(path, "0").unwrap();
            }
        }
        fs::write(root.join("herdr.log"), "").unwrap();
        refresh(root);
        let reports = writes(root);
        for (pane, quota) in [("w1:p1", "7d 88%"), ("w1:p2", "7d 27%")] {
            let report = reports
                .iter()
                .find(|args| args[2] == pane)
                .unwrap()
                .join(" ");
            assert!(report.contains(quota), "{report}");
        }
    }
}

#[test]
fn same_org_auth_change_cannot_reuse_last_good_seat_quota() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    setup(root);
    refresh(root);
    fs::write(
        root.join("a/auth.json"),
        json!({"tokens":{"account_id":"account-a"},"synthetic_seat":"new"}).to_string(),
    )
    .unwrap();
    fs::write(root.join("fail-fetch"), "").unwrap();
    fs::write(root.join("herdr.log"), "").unwrap();
    refresh(root);
    let reports = writes(root);
    for pane in ["w1:p1", "w1:p3"] {
        let report = reports
            .iter()
            .find(|args| args[2] == pane)
            .unwrap()
            .join(" ");
        assert!(report.contains("N/A"), "{report}");
        assert!(!report.contains("88%"), "{report}");
    }
    assert!(reports
        .iter()
        .find(|args| args[2] == "w1:p2")
        .unwrap()
        .join(" ")
        .contains("27%"));
    assert_eq!(
        fs::read_to_string(root.join("codex.log"))
            .unwrap()
            .lines()
            .count(),
        3
    );
}

#[test]
fn invalid_allowlists_never_fall_back_to_the_default_home() {
    for config in [
        "not json",
        "[]",
        "[\"relative\"]",
        "[\"/nonexistent-synthetic-home\"]",
    ] {
        let dir = tempdir().unwrap();
        let root = dir.path();
        setup(root);
        fs::rename(root.join("a"), root.join("wrong-home")).unwrap();
        fs::write(root.join("config/codex-homes"), config).unwrap();
        refresh(root);
        assert!(!root.join("codex.log").exists());
        assert!(writes(root)
            .iter()
            .all(|report| report.join(" ").contains("identity unavailable")));
    }
}

#[test]
fn mismatched_headers_and_external_session_symlinks_fail_closed() {
    for symlink in [false, true] {
        let dir = tempdir().unwrap();
        let root = dir.path();
        setup(root);
        if symlink {
            fs::rename(root.join("a/sessions"), root.join("outside-sessions")).unwrap();
            std::os::unix::fs::symlink(root.join("outside-sessions"), root.join("a/sessions"))
                .unwrap();
        } else {
            fs::write(
                root.join(format!("a/sessions/rollout-2099-01-01-{A}.jsonl")),
                json!({"type":"session_meta","payload":{"id":B}}).to_string(),
            )
            .unwrap();
        }
        refresh(root);
        let reports = writes(root);
        let a = reports
            .iter()
            .find(|args| args[2] == "w1:p1")
            .unwrap()
            .join(" ");
        assert!(a.contains("identity unavailable"), "{a}");
        assert!(!a.contains("88%"));
    }
}

#[test]
fn native_and_pi_codex_panes_refresh_their_separate_scopes() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    setup(root);
    account(root, "wrong-home", "account-pi", &[]);
    fs::write(
        root.join("wrong-auth"),
        json!({"tokens":{"account_id":"account-pi"}}).to_string(),
    )
    .unwrap();
    fs::create_dir_all(root.join("pi/sessions")).unwrap();
    fs::write(
        root.join("pi/auth.json"),
        json!({"openai-codex":{"type":"oauth","accountId":"account-pi"}}).to_string(),
    )
    .unwrap();
    let session = root.join("pi/sessions/session-codex-usage.jsonl");
    fs::write(
        &session,
        include_str!("fixtures/pi/session-codex-usage.jsonl"),
    )
    .unwrap();
    let mut inventory: Value =
        serde_json::from_str(&fs::read_to_string(root.join("inventory.json")).unwrap()).unwrap();
    inventory["agents"].as_array_mut().unwrap().push(
        json!({"agent":"pi","pane_id":"w1:p4","agent_session":{"kind":"path","value":session}}),
    );
    fs::write(root.join("inventory.json"), inventory.to_string()).unwrap();
    refresh(root);
    let cache = CacheStore::new(root.join("cache"));
    let canonical = cache
        .load(herdr_agent_quota::model::Provider::Codex)
        .unwrap()
        .unwrap();
    assert_eq!(canonical.account_id.as_deref(), Some("account-pi"));
    let calls = fs::read_to_string(root.join("codex.log")).unwrap();
    assert_eq!(calls.lines().count(), 3, "{calls}");
    let reports = writes(root);
    // A narrow --provider refresh publishes only that native harness. Pi's
    // next event consumes the independently refreshed canonical snapshot.
    assert_eq!(reports.len(), 3);
    assert!(!reports.iter().any(|args| args[2] == "w1:p4"));
    assert!(reports
        .iter()
        .find(|args| args[2] == "w1:p1")
        .unwrap()
        .join(" ")
        .contains("88%"));
}

#[test]
fn one_account_lease_does_not_block_another_account_refresh() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    setup(root);
    refresh(root);
    let mut held_lease = None;
    for entry in fs::read_dir(root.join("cache")).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_some_and(|ext| ext == "refresh") {
            fs::write(&path, "0").unwrap();
        }
        if path.extension().is_some_and(|ext| ext == "json") {
            let snapshot: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
            if snapshot["account_id"] == "account-a" {
                let lease = fs::File::options()
                    .read(true)
                    .write(true)
                    .open(path.with_extension("refresh.lock"))
                    .unwrap();
                lease.lock().unwrap();
                held_lease = Some(lease);
            }
        }
    }
    assert!(held_lease.is_some());
    fs::write(root.join("codex.log"), "").unwrap();
    refresh(root);
    assert_eq!(
        fs::read_to_string(root.join("codex.log")).unwrap(),
        "account-b\n"
    );
    drop(held_lease);
}

#[test]
fn scoped_omission_does_not_restore_an_expired_window_or_unstamped_rollout_quota() {
    use herdr_agent_quota::model::{ProviderSnapshot, ResetAt, WindowKind};
    let dir = tempdir().unwrap();
    let root = dir.path();
    setup(root);
    refresh(root);
    for entry in fs::read_dir(root.join("cache")).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_some_and(|ext| ext == "refresh") {
            fs::write(&path, "0").unwrap();
        }
        if path.extension().is_some_and(|ext| ext == "json") {
            let mut snapshot: ProviderSnapshot =
                serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
            for window in &mut snapshot.windows {
                if window.kind == WindowKind::Weekly {
                    window.resets_at = Some(ResetAt::from_unix_seconds(1));
                }
            }
            fs::write(path, serde_json::to_vec(&snapshot).unwrap()).unwrap();
        }
    }
    let path = root.join(format!("a/sessions/rollout-2099-01-01-{A}.jsonl"));
    let header = fs::read_to_string(&path).unwrap();
    let observation = json!({"type":"event_msg","payload":{"type":"token_count","rate_limits":{"secondary":{"used_percent":99,"window_minutes":10080,"resets_at":4000500000u64}}}});
    fs::write(path, format!("{header}\n{observation}\n")).unwrap();
    fs::write(root.join("omit-week"), "").unwrap();
    refresh(root);
    for entry in fs::read_dir(root.join("cache")).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_some_and(|ext| ext == "json") {
            let snapshot: ProviderSnapshot =
                serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
            assert!(snapshot.window(WindowKind::FiveHour).is_some());
            assert!(snapshot.window(WindowKind::Weekly).is_none());
        }
    }
}
