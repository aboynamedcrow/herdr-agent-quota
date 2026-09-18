//! Optional consumer adapter for one shared account-usage cache.
use crate::cli::PercentStyle;
use crate::herdr::{AgentPane, PaneQuotaUpdate, PaneTokens};
use crate::model::{Harness, Provider, ProviderSnapshot};
use crate::presentation::MetadataTokens;
use crate::{prefs, process, route};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::time::Duration;

#[derive(Default, Deserialize)]
pub struct Entry {
    snapshot: Option<ProviderSnapshot>,
    #[serde(default)]
    reason: String,
    key: Option<String>,
    #[serde(skip)]
    codex: Option<crate::codex_accounts::CodexAccount>,
}

pub fn enabled() -> bool {
    prefs::read("shared-usage-command").is_some()
}

pub fn owns(pane: &AgentPane) -> bool {
    enabled() && matches!(pane.harness, Harness::Claude | Harness::Codex)
}

pub fn load(panes: &[AgentPane]) -> BTreeMap<String, Entry> {
    let Some(command) = prefs::read("shared-usage-command") else {
        return BTreeMap::new();
    };
    let mut accounts = BTreeMap::new();
    let requests: Vec<_> = panes
        .iter()
        .filter(|p| owns(p))
        .map(|p| {
            let account = route::resolve_with_identity(p).codex;
            let home = account.as_ref().map(|a| a.home.clone());
            accounts.insert(p.pane_id.clone(), account);
            serde_json::json!({"pane_id": p.pane_id, "provider": p.harness.billing(), "home": home})
        })
        .collect();
    if requests.is_empty() {
        return BTreeMap::new();
    }
    let result = serde_json::to_vec(&requests).ok().and_then(|input| {
        process::run_shell_with_deadline(&command, &input, Duration::from_secs(8)).ok()
    });
    let mut entries: BTreeMap<String, Entry> = result
        .filter(|r| !r.timed_out && r.exit_code == Some(0) && r.stdout.len() <= 1024 * 1024)
        .and_then(|r| serde_json::from_slice(&r.stdout).ok())
        .unwrap_or_default();
    for (id, entry) in &mut entries {
        entry.codex = accounts.remove(id).flatten();
    }
    entries
}

pub fn tokens(
    cache: &crate::cache::CacheStore,
    pane: &AgentPane,
    mut resolved: route::ResolvedPane,
    entry: Option<&Entry>,
    now: u64,
    style: PercentStyle,
) -> PaneTokens {
    let provider = pane.harness.billing().unwrap_or(Provider::Codex);
    let mut local = ProviderSnapshot::new(provider, vec![], now);
    if let Some(session) = pane.session.as_ref().and_then(|s| s.id()) {
        if let Some(account) = resolved.codex.as_ref().filter(|a| a.is_current()) {
            crate::providers::codex::enrich_local_sessions_at(
                &mut local,
                &account.home,
                &[session.to_string()],
            );
        } else if pane.harness == Harness::Claude {
            if let Ok(Some(observation)) = cache.load_statusline_observation(Provider::Claude) {
                local = observation.snapshot;
            }
        }
        resolved.context = local.session_contexts.get(session).cloned();
        if let Some(model) = local.session_models.get(session) {
            resolved.identity = Some(crate::herdr::PaneIdentity {
                provider: provider.display_name().to_string(),
                model: model.clone(),
            });
        }
    }

    let snapshot = entry.and_then(|e| e.snapshot.as_ref()).filter(|s| {
        s.provider == provider
            && s.fetched_at_unix <= now
            && now - s.fetched_at_unix <= 300
            && !s.windows.is_empty()
            && s.windows.iter().all(|w| w.is_current(now))
            && (pane.harness != Harness::Codex
                || entry.and_then(|e| e.codex.as_ref()).is_some_and(|a| {
                    a.is_current()
                        && resolved.codex.as_ref().is_some_and(|current| {
                            current.home == a.home && current.generation() == a.generation()
                        })
                }))
    });
    let values = snapshot
        .map(|s| {
            let mut combined = s.clone();
            combined.session_contexts = local.session_contexts.clone();
            combined.session_models = local.session_models.clone();
            MetadataTokens::from_snapshot_for_pane(
                &combined,
                now,
                pane.session.as_ref().and_then(|s| s.id()),
                style,
            )
        })
        .unwrap_or_else(|| {
            MetadataTokens::unavailable(
                provider,
                entry
                    .map(|e| e.reason.as_str())
                    .filter(|s| !s.is_empty())
                    .unwrap_or("Shared usage unavailable"),
            )
        });
    PaneTokens {
        pane_id: pane.pane_id.clone(),
        quota: PaneQuotaUpdate::Replace(Box::new(values)),
        identity: resolved.identity,
        context: resolved.context,
        notification_key: entry
            .and_then(|e| e.key.clone())
            .map(|k| format!("harkness:{k}")),
    }
}
