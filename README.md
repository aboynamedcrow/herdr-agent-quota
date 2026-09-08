# herdr-agent-quota

Model, context, prompt-cache usage, and subscription quota in Herdr's Agent sidebar.

[![CI](https://github.com/levi-qiao/herdr-agent-quota/actions/workflows/ci.yml/badge.svg)](https://github.com/levi-qiao/herdr-agent-quota/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

[简体中文](README.zh-CN.md)

<table>
<tr><th>packed (default)</th><th>stacked</th></tr>
<tr>
<td valign="top"><img src="docs/screenshots/sidebar-packed.png" alt="Packed sidebar" width="284"></td>
<td valign="top"><img src="docs/screenshots/sidebar-stacked.png" alt="Stacked sidebar" width="177"></td>
</tr>
</table>

The plugin preserves Herdr's native machine/workspace/tab row, custom styles,
and worktree grouping. The branded provider/model line is the agent identity;
the native `agent` row is omitted so `grok` does not sit above `Grok/grok-4.6`.
Optional quota ordering and low-quota notifications are disabled by default.
Empty fields collapse; percentages can show remaining or used quota.

## Install and upgrade

Requires **Herdr 0.9.0+**, the Rust toolchain pinned in `rust-toolchain.toml`,
macOS or Linux, and a supported agent CLI.

```sh
git clone https://github.com/levi-qiao/herdr-agent-quota.git
cd herdr-agent-quota
./install.sh
```

To enable a subset, use `./install.sh --agent claude,codex,omp`.
Existing sessions need restarting only when newly installed hooks or Herdr
integrations must be loaded.

Upgrade from the repository directory:

```sh
git pull --ff-only
./install.sh
```

Upgrades retain saved preferences, repair managed configuration, refresh quota,
and restore background updates automatically. No cache deletion or watcher
management is required. Changes to the Herdr server connection are adopted by
the watcher automatically.

## Settings

Press `prefix+shift+q`, or run the following if that key is already assigned:

```sh
herdr plugin pane open --plugin herdr-agent-quota --entrypoint settings --focus
```

<img src="docs/screenshots/settings.png" alt="Agent quota settings" width="760">

| Setting | Options |
| --- | --- |
| Percentages | Remaining or used; colors always indicate remaining headroom |
| Layout | `packed` groups related fields; `stacked` gives each field a row |
| Row gap | Zero or one blank line between agents |
| Watch interval | 30 seconds–1 hour; default 60 seconds |
| Fields | Topic, model, cache, TTL, context, short/long quota |
| Brand colors | On or off |
| Agent order | Herdr default or lowest remaining quota first |
| Low quota alert | Off or a threshold from 1% to 100% |
| Agents | Claude, Codex, Grok, Agy, OpenCode, Pi, OMP, Devin |

Use arrows or Space to edit, `a` to apply, and `q` to close.
Installer options are also available through `./install.sh --help`.

### Native Codex account homes

Native Codex panes are matched by Herdr's exact session UUID to a rollout
header in an allowed Codex home. Without configuration, the plugin uses its
process's `CODEX_HOME`, or `~/.codex` when unset. A missing session or account
identity displays `N/A`; it never borrows another home's quota.

For multiple accounts, put a JSON array of absolute account-home paths in the
`codex-homes` file under the plugin's `HERDR_PLUGIN_CONFIG_DIR`. Obtain the
paths from your account manager's resolver. This opt-in list replaces the
default home; include every home whose native panes should receive quota.
The file is an advanced preference, separate from the settings popup. Herdr
plugin actions run in the server's environment, so exporting `CODEX_HOME`
around `herdr plugin action invoke` does not configure them.

The list permits at most 32 homes and 64 KiB of JSON. Relative paths, malformed
configuration, unreadable homes, duplicate session matches across homes, and
session links escaping a home fail closed. A symlinked `auth.json` also fails
closed, even when its target is a regular file inside the same home: the
resolver requires a regular credential file and does not follow credential
links. Such panes display `N/A` with identity unavailable. Removing the
allowlist file restores the single-home default; a full uninstall removes it too.

Each home uses a separate collector, cache, refresh lease, and 60-second
debounce. Panes sharing that home's unchanged credentials share one request.
Storage paths stay fixed per canonical home across account changes and token
rotation: one snapshot, one refresh marker, and one lease file. The snapshot
and refresh marker carry an opaque credential-generation stamp, so a changed
generation cannot reuse quota, omitted windows, or debounce from the old one.
Any change to `auth.json` invalidates its cached attribution, even when the
organization account ID is unchanged. This deliberately includes routine
token rotation: a fresh successful collection is required before showing
quota again. A file-authenticated ChatGPT account is required; keychain-only
and API-key authentication are not attributed by this resolver.

Low-quota warnings remember each native Codex home/account independently.
Another account's healthy quota, an absent pane, or unavailable identity does
not rearm a low account. Only an observed recovery above the threshold rearms
it. Warning identity stays stable across token rotation; other collectors
retain their existing provider-level warning behavior.

Keep each home dedicated to its account while native panes are running. The
session-to-home mapping does not identify credentials retained inside an
already-running CLI after an in-place login change. Quota comes from the
home's current app-server account. Session rollouts supply bounded local
model/context diagnostics, never replacement account quota. A failed fetch
keeps the last good snapshot only for unchanged credentials; a successful
API reading replaces all windows, including omitted ones. The sidebar uses the
account-wide `codex` limit pool only; model-specific pools never fill a missing
5h or 7d window. Legacy responses with an absent, null, or empty pool map use
their default pool; malformed maps do not authorize a fallback.

The source verification recipe is [verify-herdr-agent-quota](docs/verify-herdr-agent-quota/SKILL.md).

## Data sources and limits

| Agent | Quota source | Attribution |
| --- | --- | --- |
| Codex | Codex app-server; 5h and/or 7d | Exact native session in one allowed account home |
| Grok | CLI billing endpoint; 7d or 30d | Current CLI credentials |
| Devin | CLI usage endpoint; 1d and 7d | Current CLI credentials |
| Claude Code | StatusLine; 5h and 7d | Exact session observation |
| Agy / Antigravity | StatusLine; 5h and 7d | Exact session and identifiable model pool |
| OpenCode | OpenCode Go usage endpoint | Go credential; confirmed PAYG routes have no subscription quota |
| Pi | Canonical Codex quota | Only when the recorded account matches |
| OMP | `omp usage --json --provider <id>` | Reported account matching the session's credential pin |

Quota windows retain their provider's meaning. Model, context, and cache data
come from the identified session when available. `ttl≈` marks an estimated
prompt-cache lifetime, not a guaranteed expiry. Topic extraction uses only the
named pane's visible screen and preserves the last topic when it scrolls away.

All supported working agents participate in one background watcher. Requests
are debounced for 60 seconds, including a final refresh after a turn settles.
OMP additionally retains its own five-minute usage cache. Idle panes sharing a
verified quota source receive the same reading.

Native Codex attributes each pane's exact session to one allowed account home
and uses that home's current account. Grok and Devin collectors follow the
plugin's current CLI credentials, without separate account attribution per pane.
Claude/Agy do not report a reliable serving account ID, so their observations
are not shared across sessions. Unknown
identity or model-pool attribution does not produce a guessed quota. Failed
requests preserve the last verified reading for that same account; they do not
turn failures into zero usage. Native Codex also requires unchanged credentials
to retain that reading.

## Troubleshooting

| Symptom | Check |
| --- | --- |
| Session data is missing | Run `herdr integration status`; load missing integrations before restarting the affected agent |
| Claude/Agy quota is missing | Send a turn so the session's StatusLine produces an observation |
| OMP quota is missing | Check `omp usage --json --redact --provider <id>` |
| Devin quota is missing | Check the CLI login and `DEVIN_CREDENTIALS_FILE` if customized |
| Rows are missing | Run the configure action below to repair managed configuration |
| Packed rows are truncated | Select `stacked` |

```sh
herdr plugin action invoke refresh --plugin herdr-agent-quota
herdr plugin action invoke configure --plugin herdr-agent-quota
```

Uninstall everything with `./uninstall.sh`, or remove a subset with
`./uninstall.sh --agent grok`. Configuration changes are reversible; user-owned
settings and other agents remain intact.

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for development and validation,
[SECURITY.md](SECURITY.md) for data handling and vulnerability reports, and
[CHANGELOG.md](CHANGELOG.md) for release notes. Dated investigations are indexed
in [docs/README.md](docs/README.md).

## License

[MIT](LICENSE). Not affiliated with Herdr or the supported AI providers.
