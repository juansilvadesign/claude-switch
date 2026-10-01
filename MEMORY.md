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
- **Next:** Stage B, Codex profiles (one `CODEX_HOME` each), then Stage C, Antigravity profiles (one fake HOME each). The TUI usage panel (Phase 3) follows. Then Phase 4, a status line, which needs an incremental path faster than rewriting the whole ledger.

## 📚 Detailed history

⚠️ **This repository is PUBLIC, so the full internal history is deliberately NOT kept here.** This file carries the sanitized technical state only.

The complete record lives in the private `ai-synthesizer` workspace at `knowledge/projects/_memory/project_claude_switch_fork.md` — session-by-session, including the parts that must not be published (hosting account details, client agreements, internal IDs). Folded there 2026-08-17.
