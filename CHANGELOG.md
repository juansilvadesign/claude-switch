# Changelog

Notable changes to this fork of [claude-switch](https://github.com/Abhishek21k/claude-switch).

Format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

A profile is a **config environment**, not an identity. Its name is a local label you choose; the Claude account inside it comes from authentication and nothing else. Several behaviors in this release exist to make that true in practice rather than only in principle.

### Fixed

- **Adding an account in the TUI no longer silently clones the account you already had.** Pressing `a` used to copy `~/.claude` wholesale — credentials included — so every "new account" came back with the default account's email, by construction. `a` now asks for a name and then which operation you meant: copy the current session, or log in to a different account. The CLI and the first-run screen already offered this choice; the normal TUI did not.
- **A profile name that was already taken no longer tears down the TUI.** The error surfaces as an in-app message instead of propagating out of the event loop.
- **Symlinked content in `~/.claude` no longer aborts profile creation.** A symlink to a directory reported as neither file nor directory and reached a plain file copy, which failed with `Is a directory` and abandoned the whole operation. Links are now recreated as links, with relative targets resolved to absolute so they still resolve from the profile's new location. A dangling link stays dangling rather than being fatal.
- **Seeding without history no longer copies `transcripts/` or `plans/`.** Both hold conversation content (verbatim prompts, tool calls and plan-mode documents), and a profile seeded for another account must not inherit either. `--include-history` still brings them along. `debug/`, `usage-data/` and `stats-cache.json` are never copied, because they are the source account's usage accounting. Ported from upstream `2308675`.
- **The refresh confirmation now says when it will delete the profile's conversation history.** `r` reseeds without history, so transcripts and prompt history are lost even on a same-account refresh, which the dialog used to present as safe. It now names the loss and turns red. Ported from upstream `1ba8402`.

### Added

- **Offline usage billing estimates.** `list` shows a `30D $` column, and `info` shows today, seven-day and 30-day tokens and dollars. Per-token profiles can use owner-set rates; subscription profiles can show an owner-set fee and estimated weekly capacity. Exact model aliases map gateway transcript names to editable list prices. Settings stay in a private file and no billing service is contacted.

- **Codex profiles.** Log in with a separate `CODEX_HOME` per profile, seeded only with warm configuration. `use`, aliases, list, info and the TUI dispatch by tool; Codex identity and plan type come from local JWT claims. Claude-only key, gateway, sync, usage and limit paths exclude Codex profiles.
- **Anthropic-compatible gateway settings for API keys.** After hidden key entry, the CLI and TUI accept a base URL or provider settings JSON. Credential fields in pasted JSON are discarded; non-credential settings can be saved per normalized URL and managed with `cswitch gateway list` and `cswitch gateway forget`. Clearing a managed key removes its base URL in the same settings edit, and seeded or copied profiles drop inherited gateway settings.
- **Anthropic Console login and per-profile API keys.** Console profiles can be created from the CLI or Add menu. `cswitch key set` stores a private key outside the profile and installs a helper in `settings.json`; `key clear` removes it and reports the fallback. CLI and TUI views show the authentication source, with `api` in the list for API-billed profiles.
- **Offline token ledger** in `cswitch usage`, with incremental transcript ingest, cross-file and sidechain deduplication, project labels and attribution explanations, editable API-equivalent rates, and cost-state verification. Profile removal retains its ledger rows unless `--purge-usage` is specified.
- **Plan-limit snapshots** in `cswitch list`, `cswitch info`, and the TUI details panel, with cache age, reset markers, and Claude Code's severity flags.
- **`cswitch sync`** links shared skills into one or all profiles, with dry-run previews and explicit adoption of diverged copies after a backup.
- **New profiles are seeded with your warm setup before authenticating.** Settings, skills, and per-project trust are copied first, then every trace of the previous account is removed so Claude runs its normal login. An empty profile directory is a blank Claude Code: `CLAUDE_CONFIG_DIR` relocates `.claude.json` too, so MCP servers drop back to pending approval and per-directory trust disappears.
- **Same-account detection.** Authenticating as an account another profile already holds is reported by name, instead of looking like a distinct account was added. It is a note, not an error — two profiles can intentionally isolate settings for one account.
- **A confirmation before `r` overwrites a profile**, naming the account it currently holds and the account it will become, and turning red when those differ.
- **A warning when refreshing or deleting a profile another session may have open.** Profiles run concurrently — `CLAUDE_CONFIG_DIR` is read per process, so each terminal is bound to whichever profile launched it. Destructive operations can therefore land on files a live session is using. The check is advisory: it can tell that a session wrote to the profile, not whether that terminal is still open.
- **`--include-history`** to opt into copying conversation transcripts and prompt history.

### Changed

- **Shell aliases run `cswitch use`.** They sync skills before launch and pass Claude flags through.
- **Unix launches use `exec`.** Claude takes over the process, preserving its PID, signals, and exit code.
- **New profiles link user-level skills individually instead of copying them.** Their account-managed `skills/synced/` directory remains separate.
- **Registry writes are atomic.** A complete temporary file is renamed over `registry.json`.
- **Login runs `claude auth login`, and the resulting account is read back from `claude auth status --json`.** Account identity is never taken from user input or from stale config metadata.
- **Conversation history is no longer copied by default.** Transcripts, prompt history, and machine-local caches are excluded unless `--include-history` is passed; separate sessions per profile are usually the point.
- **A clean exit from the login flow is no longer treated as success.** The session is re-checked afterwards, so a dismissed browser tab cannot register a profile with no credentials behind it.
- **A failed or cancelled login removes only a directory that attempt created**, and leaves the registry untouched either way.
- **Synced with upstream v0.2.0.** The version is now 0.2.0. The rest of upstream's changes are this fork's own pull requests #1–#4, so upstream's `main` is recorded as merged without changing any file.

### Notes

- **`cswitch` cannot choose which account a browser authorizes.** `claude auth login` delegates that to your claude.ai session, so a signed-in browser authorizes that account with no picker. `--email` pre-fills the login page; it does not override the session. Sign out or use a private window to authenticate as someone else. The tool now reports when this happens rather than hiding it.
- **Account email is read-only.** There is deliberately no field for typing one in: that would relabel copied credentials without changing which account they authenticate as.
- **Project-level skills do not follow a profile.** Skills in `~/.claude/skills/` are linked individually; skills in a repository's own `.claude/skills/` belong to that repository and load from wherever you launch Claude, identically under every profile.
- **MCP authorizations do not transfer.** Server definitions are copied, but an OAuth grant belongs to the account that gave it, so a server may ask a new account to authenticate again.
