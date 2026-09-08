//! Native Codex pane attribution from exact session ids in explicitly allowed homes.
//! No process environments, credential discovery, or unrelated transcripts are read.
use crate::model::{BillingTarget, ProviderSnapshot};
use crate::providers::codex;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::fs;
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub struct CodexAccount {
    pub home: PathBuf,
    pub account_id: String,
    auth_digest: [u8; 32],
}

impl CodexAccount {
    pub fn target(&self) -> BillingTarget {
        BillingTarget::codex_account(&self.home)
    }

    pub fn generation(&self) -> String {
        let mut hash = Sha256::new();
        hash.update(self.warning_identity().as_bytes());
        hash.update([0]);
        hash.update(self.auth_digest);
        format!("{:x}", hash.finalize())
    }

    /// A different home/account cannot rearm this account's warning. Token
    /// rotation leaves this identity unchanged; no raw identity is published.
    pub fn warning_identity(&self) -> String {
        let mut hash = Sha256::new();
        hash.update(self.target().cache_identity().as_bytes());
        hash.update([0]);
        hash.update(self.account_id.as_bytes());
        format!("codex-account:{:x}", hash.finalize())
    }

    pub fn matches_snapshot(&self, snapshot: &ProviderSnapshot) -> bool {
        snapshot.account_id.as_deref() == Some(self.account_id.as_str())
            && snapshot.credential_generation.as_deref() == Some(self.generation().as_str())
    }

    pub fn is_current(&self) -> bool {
        auth_digest(&self.home.join("auth.json")) == Some(self.auth_digest)
    }
}

/// An explicit JSON array replaces the default home. In Herdr this is a
/// persistent preference; an invocation's exported environment does not reach
/// plugin actions. Paths must come from the user's account resolver, never a
/// guessed email/workspace slug. A malformed allowlist grants no fallback.
fn allowed_homes() -> Option<Vec<PathBuf>> {
    if let Some(config) = std::env::var_os("HERDR_PLUGIN_CONFIG_DIR") {
        let path = PathBuf::from(config).join(crate::prefs::CODEX_HOMES);
        match fs::metadata(&path) {
            Ok(metadata) => {
                if !metadata.is_file() || metadata.len() > 64 * 1024 {
                    return None;
                }
                let file = fs::File::open(path).ok()?;
                let homes: Vec<PathBuf> = serde_json::from_reader(file.take(64 * 1024)).ok()?;
                return (homes.len() <= 32).then_some(homes);
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return None,
        }
    }
    Some(vec![codex::codex_home().ok()?])
}

pub fn resolve(session_id: Option<&str>) -> Option<CodexAccount> {
    resolve_in(session_id?, &allowed_homes()?)
}

fn resolve_in(session_id: &str, homes: &[PathBuf]) -> Option<CodexAccount> {
    // Native thread ids are UUIDs. This also prevents glob/path-shaped or
    // short substring matches from opening an unrelated rollout.
    if session_id.len() != 36
        || !session_id.bytes().enumerate().all(|(index, byte)| {
            if [8, 13, 18, 23].contains(&index) {
                byte == b'-'
            } else {
                byte.is_ascii_hexdigit()
            }
        })
    {
        return None;
    }
    let mut canonical = BTreeSet::new();
    for home in homes {
        if !home.is_absolute() {
            return None;
        }
        // An unreadable allowlisted home could contain a duplicate session.
        canonical.insert(home.canonicalize().ok()?);
    }
    let mut matched = None;
    for home in canonical {
        if contains_session(&home, session_id)? {
            if matched.is_some() {
                return None;
            }
            matched = Some(home);
        }
    }
    let home = matched?;
    let path = home.join("auth.json");
    let auth_digest = auth_digest(&path)?;
    let account_id = codex::account_id_from_auth(&path)?;
    let account = CodexAccount {
        home,
        account_id,
        auth_digest,
    };
    account.is_current().then_some(account)
}

/// Account ids may identify an organization rather than a seat. Conservatively
/// invalidate a home's cache on any credential-file change, including refreshes.
/// Hash bounded chunks; never retain or publish credential strings or the digest.
fn auth_digest(path: &Path) -> Option<[u8; 32]> {
    // Deliberately reject every credential symlink, even within the home.
    // Session-directory containment does not grant credential indirection.
    if !fs::symlink_metadata(path).ok()?.is_file() {
        return None;
    }
    let mut file = fs::File::open(path).ok()?;
    let metadata = file.metadata().ok()?;
    if !metadata.is_file() || metadata.len() > 1024 * 1024 {
        return None;
    }
    let mut hash = Sha256::new();
    let mut remaining = 1024 * 1024usize;
    let mut buffer = [0; 8192];
    loop {
        let count = file.read(&mut buffer).ok()?;
        if count == 0 {
            return Some(hash.finalize().into());
        }
        remaining = remaining.checked_sub(count)?;
        hash.update(&buffer[..count]);
    }
}

fn contains_session(home: &Path, id: &str) -> Option<bool> {
    let mut pending = vec![
        (home.join("sessions"), 0),
        (home.join("archived_sessions"), 0),
    ];
    let suffix = format!("-{id}.jsonl");
    let mut remaining = 100_000usize;
    let mut found = false;
    while let Some((directory, depth)) = pending.pop() {
        if depth > 8 {
            return None;
        }
        let entries = match fs::read_dir(&directory) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(_) => return None,
        };
        // Root session directories may be symlinks; they must stay in this home.
        if !directory.canonicalize().ok()?.starts_with(home) {
            return None;
        }
        for entry in entries {
            remaining = remaining.checked_sub(1)?;
            let entry = entry.ok()?;
            let kind = entry.file_type().ok()?;
            if kind.is_dir() {
                pending.push((entry.path(), depth + 1));
            } else if entry
                .file_name()
                .to_str()
                .is_some_and(|name| name.starts_with("rollout-") && name.ends_with(&suffix))
            {
                if !kind.is_file() || !entry.path().canonicalize().ok()?.starts_with(home) {
                    return None;
                }
                // Only the matching file's first line is consumed. Unknown
                // fields (including instructions) are skipped by serde.
                if !session_header_matches(&entry.path(), id) {
                    return None;
                }
                found = true;
            }
        }
    }
    Some(found)
}

fn session_header_matches(path: &Path, id: &str) -> bool {
    #[derive(Deserialize)]
    struct Header {
        #[serde(rename = "type")]
        kind: String,
        payload: HeaderPayload,
    }
    #[derive(Deserialize)]
    struct HeaderPayload {
        id: String,
    }
    let Ok(file) = fs::File::open(path) else {
        return false;
    };
    // Stop at the newline without materializing the remainder of the rollout.
    let bytes = BufReader::new(file)
        .take(256 * 1024)
        .bytes()
        .take_while(|byte| !matches!(byte, Ok(b'\n')))
        .collect::<std::io::Result<Vec<_>>>();
    bytes
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Header>(&bytes).ok())
        .is_some_and(|header| header.kind == "session_meta" && header.payload.id == id)
}
