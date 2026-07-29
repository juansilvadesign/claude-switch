# Changelog

Notable changes to this fork of [claude-switch](https://github.com/Abhishek21k/claude-switch).

Format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

A profile is a **config environment**, not an identity. Its name is a local label you choose; the Claude account inside it comes from authentication and nothing else. Several behaviors in this release exist to make that true in practice rather than only in principle.

### Fixed

- **Adding an account in the TUI no longer silently clones the account you already had.** Pressing `a` used to copy `~/.claude` wholesale — credentials included — so every "new account" came back with the default account's email, by construction. `a` now asks for a name and then which operation you meant: copy the current session, or log in to a different account. The CLI and the first-run screen already offered this choice; the normal TUI did not.
- **A profile name that was already taken no longer tears down the TUI.** The error surfaces as an in-app message instead of propagating out of the event loop.
- **Symlinked content in `~/.claude` no longer aborts profile creation.** A symlink to a directory reported as neither file nor directory and reached a plain file copy, which failed with `Is a directory` and abandoned the whole operation. Links are now recreated as links, with relative targets resolved to absolute so they still resolve from the profile's new location. A dangling link stays dangling rather than being fatal.

### Added

- **New profiles are seeded with your warm setup before authenticating.** Settings, skills, and per-project trust are copied first, then every trace of the previous account is removed so Claude runs its normal login. An empty profile directory is a blank Claude Code: `CLAUDE_CONFIG_DIR` relocates `.claude.json` too, so MCP servers drop back to pending approval and per-directory trust disappears.
- **Same-account detection.** Authenticating as an account another profile already holds is reported by name, instead of looking like a distinct account was added. It is a note, not an error — two profiles can intentionally isolate settings for one account.
- **A confirmation before `r` overwrites a profile**, naming the account it currently holds and the account it will become, and turning red when those differ.
- **A warning when refreshing or deleting a profile another session may have open.** Profiles run concurrently — `CLAUDE_CONFIG_DIR` is read per process, so each terminal is bound to whichever profile launched it. Destructive operations can therefore land on files a live session is using. The check is advisory: it can tell that a session wrote to the profile, not whether that terminal is still open.
- **`--include-history`** to opt into copying conversation transcripts and prompt history.

### Changed

- **Login runs `claude auth login`, and the resulting account is read back from `claude auth status --json`.** Account identity is never taken from user input or from stale config metadata.
- **Conversation history is no longer copied by default.** Transcripts, prompt history, and machine-local caches are excluded unless `--include-history` is passed; separate sessions per profile are usually the point.
- **A clean exit from the login flow is no longer treated as success.** The session is re-checked afterwards, so a dismissed browser tab cannot register a profile with no credentials behind it.
- **A failed or cancelled login removes only a directory that attempt created**, and leaves the registry untouched either way.

### Notes

- **`cswitch` cannot choose which account a browser authorizes.** `claude auth login` delegates that to your claude.ai session, so a signed-in browser authorizes that account with no picker. `--email` pre-fills the login page; it does not override the session. Sign out or use a private window to authenticate as someone else. The tool now reports when this happens rather than hiding it.
- **Account email is read-only.** There is deliberately no field for typing one in: that would relabel copied credentials without changing which account they authenticate as.
- **Project-level skills do not follow a profile.** Skills in `~/.claude/skills/` are copied; skills in a repository's own `.claude/skills/` belong to that repository and load from wherever you launch Claude, identically under every profile.
- **MCP authorizations do not transfer.** Server definitions are copied, but an OAuth grant belongs to the account that gave it, so a server may ask a new account to authenticate again.
