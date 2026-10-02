---
name: claude-switch fork — project memory
description: Live state for the claude-switch fork — bundles shipped and what is next
type: project
---
# claude-switch fork — Project Memory

> **Migrated out of the global memory router 2026-08-16.** The router keeps a one-line stub pointing here; ⛔ new detail lands in this file, not in the router.
>
> ⚠️ **This repository is PUBLIC.** No credentials, tokens, or profile contents in this file.

## ▶ Live resume state

- ✅ **Bundle A shipped.** Per-directory switching (Bundle B) was never built. It was parked on 2026-09-27 because both profiles are run from the same directory.
- ✅ **Synced with upstream v0.2.0 on 2026-09-29.** The fork ported the two upstream fixes it lacked, `2308675` (seeding skips transcripts, plans and usage accounting) and `1ba8402` (refresh warns that history will be deleted), along with the 0.2.0 version bump. Upstream's `main` was then recorded as merged with `-s ours`, because the rest of it is this fork's PRs #1–#4 in a different shape.
- **If Bundle B is revived:** use a launch-time `claude` shell wrapper, a global `~/.claude-switch/routes.json` with longest-prefix matching, and let unrouted folders fall through to plain `claude`.
- ✅ **Skills sync shipped 2026-09-27.** `cswitch sync` links the shared user-level skills into every profile, and `cswitch use` syncs before it launches. Settings sync is parked.
- ✅ **Plan-limit bars shipped 2026-09-29.** `cswitch list`, `cswitch info` and the TUI detail panel show the 5-hour and 7-day windows from Claude Code's own cached snapshot in each profile's `.claude.json`. They read it offline and never write it, and they always print the snapshot's age. A window whose reset time has passed shows `reset`, and a snapshot that belongs to another account is hidden.
- ✅ **Token ledger shipped 2026-09-30.** `cswitch usage` reads every profile's transcripts offline into an incremental, deduplicated ledger in `~/.claude-switch/usage/`.
  - **Reports:** tokens and API-equivalent cost by workspace › project › session, plus `usage label`, `usage verify`, `--explain` and `--unattributed`.
  - **Storage:** counters and derived project signals only, never prompts or tool inputs.
  - **Checked against:** ccusage, and Claude Code's own per-session cost records.
- **Lessons (they cost two fix rounds):**
  - Claude Code writes one record per content block. The copies of a response share one id while `output_tokens` grows, so keep the largest copy and union the tool calls across copies, counted by `tool_use` id.
  - Title and cost-state records carry no `timestamp`.
- ✅ **Claude API keys and gateway settings (Stage A of the accounts work) shipped 2026-10-01.**
  - `cswitch login <name> --console` creates an Anthropic Console (API billing) profile.
  - `cswitch key set` and `cswitch key clear` switch a profile to the owner's own key and back, through a cswitch-managed `apiKeyHelper`. The key lives outside the profile in `~/.claude-switch/keys/` (`0600`), so seeding can never copy it.
  - **A key can come with an Anthropic-compatible gateway.**
    - After the key, a hidden step takes a base URL or the provider's settings JSON. Any token in the JSON is ignored.
    - The non-credential settings can be saved as defaults per base URL, with `cswitch gateway list` and `forget`.
    - `key clear` removes them in the same write as the helper, so a saved login never reaches the gateway.
  - Seeding now also strips `primaryApiKey` and `customApiKeyResponses`. A copied profile drops an inherited base URL.
- **Lessons (three fix rounds):**
  - A refusal test must make the refusal the *only* reason to fail.
    - A symlink that pointed at an invalid file hid the missing symlink check.
    - A fixture manifest that listed the base URL hid the unconditional base-URL strip.
  - The TUI's `k` is vim-style "up". The key actions live on `p`/`P`.
  - A hidden multi-line input needs a way out of malformed input: a blank line ends a JSON paste.
- ✅ **Codex profiles (Stage B of the accounts work) shipped 2026-10-02.**
  - `cswitch login <name> --tool codex` creates a profile with its own `CODEX_HOME`. It is seeded from `CODEX_HOME` or `~/.codex` with five warm entries only: `config.toml`, `AGENTS.md`, `agents/`, `rules/` and `skills/`. Credentials, sessions, the sqlite state and `packages/` are never copied.
  - The login runs `codex login`, then `codex login status`, and trusts only their exit codes. The email and plan are read offline from the `id_token` claims in `auth.json`.
    - A failed login or status check registers nothing, and the refusal names the step that failed.
    - A login whose claims can't be read is still registered, without an email.
  - `use`, the aliases (`codex-<name>`), `list`, `info` and the TUI dispatch by tool. Keys, gateways, skills sync, usage, plan limits and refresh stay Claude-only.
  - The registry gains a `tool` field. A missing field loads as Claude. An unknown value is kept verbatim on save, and that profile refuses every action.
- **Release notes:**
  - ⛔ **Remove the Codex profiles before you downgrade.** A build without Codex support treats a Codex profile as Claude: `use` launches `claude` in the Codex home, `sync --all` links Claude skills into it, `usage` ingests it, and the next registry save drops `tool`. Remove the Codex profiles or restore `registry.json` first.
  - Codex 0.158 creates `packages/` and `app-server-daemon/` itself in a fresh `CODEX_HOME`, so leaving them out of the seed doesn't break the first launch.
  - `~/.codex/log/` is empty on Codex 0.158, whose logs moved to sqlite. The live-session warning reads `sessions/` and `shell_snapshots/`.
  - A `skills/` directory made mostly of links keeps its nested links when seeded.
- **Lessons (two fix rounds):**
  - A hotkey on a screen with a text field takes that character away from the field. The first-run screen bound `a`, so a name containing `a` jumped to another flow.
  - A display rule can break output that isn't a display. "Every line ≤ 120 columns" made long-named profiles lose their shell alias.
  - An enum that folds unknown values into one variant rewrites them on the next save. Keep the raw string.
  - A refusal must name the step that failed. One message for both a failed login and a failed status check was false whenever `auth.json` existed.
  - Code that spawns an external CLI needs a fake of that CLI on `PATH`. Unit tests alone never reach its wiring.
- **Next:** Stage C, Antigravity profiles (one fake HOME each). The TUI usage panel (Phase 3) follows. Then Phase 4, a status line, which needs an incremental path faster than rewriting the whole ledger.

## 📚 Detailed history

⚠️ **This repository is PUBLIC, so the full internal history is deliberately NOT kept here.** This file carries the sanitized technical state only.

The complete record lives in the private `ai-synthesizer` workspace at `knowledge/projects/_memory/project_claude_switch_fork.md` — session-by-session, including the parts that must not be published (hosting account details, client agreements, internal IDs). Folded there 2026-08-17.
