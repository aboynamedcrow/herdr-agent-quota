---
name: verify-herdr-agent-quota
description: Verify native Codex account attribution and scoped quota refresh.
---

# Codex account attribution verification

Read this repository-owned body from either native provider. It grants no
account, installation, provider request, or service-control authority.

## Source and synthetic boundary

Use the pinned toolchain in `rust-toolchain.toml`, repository-local Cargo
artifacts, and the checked-in lockfile. Run from the repository root:

```sh
cargo fmt --check
cargo test --locked
cargo clippy --release --locked
```

The real binary entrypoints under test are `refresh --provider codex`,
`startup --provider codex`, `event`, `focus`, and `watch --provider codex`.
`tests/codex_accounts.rs` supplies synthetic homes, file auth metadata,
session headers, Herdr inventory, and an app-server RPC stub. The stubs are
selected by explicit executable overrides. No provider endpoint is exercised.

Require distinct quotas for distinct homes, one fetch per shared home,
independent cache/lease/debounce state, fail-closed missing or ambiguous
identity, invalidation on auth changes including a shared organization ID,
same-credential last-good retention, and valid omitted-window retention.
Repeated synthetic rotations must retain the same three storage files per
home while collecting fresh values. Missing or mismatched generation stamps
must not restore old quota or suppress a new generation's first fetch. A
symlinked `auth.json`, including an in-home target, must remain unavailable.
Alternate low A, healthy B, and still-low A: only the first A observation
warns. Token rotation, absent panes, and unavailable identity preserve warning
state; an observed recovery followed by a new low crossing warns again.
Check default-home behavior and mixed Pi/native Codex inventory. The broader
suite covers Claude, omp, window resets, and metadata token comparison.

Herdr call logs must show no pane output read on refresh/startup/focus/watch,
only a named `--source visible --format text` read on an event, at most one
publish per pane per pass, unchanged topics when extraction is empty, and no
metadata writes on a no-op refresh. Temporary directories and stub children
are owned by the tests and removed/reaped at completion.

Record the exact HEAD, tracked binary diff SHA-256, content manifest including
untracked source/tests/docs, gate exit codes, and test totals. Classify a
failure as a harness gap, documentation drift, or product regression. Limit
each diagnosed repair to three focused iterations before checkpointing the
new evidence and reassessing; do not weaken assertions to obtain a pass.

## Separate native acceptance

Source and mocked checks cannot prove installed behavior. An authorized
operator must separately install the reviewed artifact and preference list,
verify the exact native session UUID belongs to its resolver-selected home,
and compare each account's sidebar windows with independently approved
provider observations. Verify two accounts and two panes sharing one account,
plus an unchanged refresh and an actual active watch cycle.

Use only visible/detection pane reads where the entrypoint allows them. A
human must observe any repaint test: matching content hashes and zero scroll
offsets are not proof that no repaint occurred. Record installed fingerprint,
account/session mapping without secrets, observations, and any pending gates.
Installation, account operations, provider probes, and service changes require
their own authorization. Tests never stand in for those observations.
