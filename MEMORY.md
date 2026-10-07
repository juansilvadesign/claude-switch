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
- ✅ **Offline spend and plan capacity (Stage 3 of the usage work) shipped 2026-10-05.**
  - `cswitch list` and the TUI list show a `30D $` value per profile: the spend of a per-token profile, or `~` plus the list-price value of a plan's usage.
  - `cswitch info` and the TUI detail panel show tokens and dollars for today, 7 days and 30 days, the plan fee or the rate, an effective USD per million tokens, and an estimate of how much list-price usage fits in the weekly limit.
  - **Settings** are the owner's own, in a private `usage/billing.json` (`0600`):
    - `usage plan` sets a monthly fee, and `usage rate` a flat or per-type price, scoped by model prefix;
    - `usage reset` records a free weekly limit reset, by day or by minute;
    - `usage alias` maps a gateway's model names to list prices in `rates.json`.

    Nothing is fetched from any billing service.
  - **Data:** each ingest writes an hourly rollup (`hourly.json`) and appends Claude Code's weekly limit snapshot to `limits.jsonl`. `list` and `info` only read. The TUI ingests in a background thread, and drawing never waits for it.
  - **Capacity:** a weekly window is split at each reset, whether recorded or detected as a drop of at least one point. A segment's estimate is its list-price usage divided by the highest utilization seen in it, from 20% up. A reset known only by its day gives a range.
- **Lessons (four fix rounds):**
  - A weekly limit can be reset for free in the middle of a window, without moving its reset time. Reading that week as one window doubles the estimate.
  - "Every line ≤ 120 columns" needs the widest real value in the fixture. A fresh snapshot's age was one character wider than its column.
  - A release hash belongs to one commit. A test-only commit still changes the binary; a docs-only one doesn't.
  - A cleanup can make an old test pass for the wrong reason. It happened twice:
    - once empty settings entries were pruned, the purge test's empty fixture entry vanished without the purge;
    - once an empty window gave no estimate, a test with no usage in its fixture passed whatever the code did.
  - A test must fail on its known-bad, not hang. One blocked on a channel that the same thread was to release.
  - What sits in `main`'s dispatch or in the TUI's event loop is out of a unit test's reach. Extract the decision into a function, or drive the built binary.
- ✅ **Antigravity profiles (Stage C of the accounts work) shipped 2026-10-06.** Unix only.
  - `cswitch login <name> --tool antigravity` (or `--tool agy`, or `[g]` in the TUI's Add menu) creates a profile with its own HOME at `profiles/<name>/home/`. Antigravity has no setting that moves its data directory, so `HOME` is the lever.
  - **The link farm.** Every top-level entry of the real HOME is linked into the profile's HOME, with two exceptions: `.gemini` is the profile's own real directory, and `.claude-switch` is left out because the profiles live inside it. `use` re-links on every launch. It adds links for new entries, removes only dangling links that cswitch itself recorded, and never touches an entry that isn't a link.
  - **The seed** copies seven entries from the real `.gemini`: the settings file, both MCP configuration files, the three skills directories and Antigravity's migration marker. The login token, the Gemini CLI account files, the installation ids, conversations and history are never copied.
  - **The login** runs `agy` as a child for the sign-in. It then requires a non-empty token file in the profile, and `agy models` exiting 0. `agy`'s own exit status doesn't count. The email is read offline from the token's `id_token` claim, and a login whose claim can't be read is still registered, without an email.
  - `use`, the alias (`agy-<name>`), `list`, `info` and the TUI dispatch by tool. `info` shows the profile's HOME, the number of links, the number of dangling links and the local entries.
  - **Local entries** are files that a program created in the profile's HOME, or a link that a program replaced by renaming a new file over it. They exist only in the profile. `remove` lists them and refuses without `--force`, and the TUI's delete dialog lists them.
  - Run from inside an Antigravity session, `cswitch` can't see the profile store. It says so in one line and creates nothing.
  - ⛔ No walk follows a link in the farm: not the live-session warning, not the sizes, not `remove`, not the seed. A careless walk would reach the whole real HOME.
- **Release notes:**
  - A build from the Codex stage lists an Antigravity profile as `unknown` and refuses to launch it. It keeps the profile's `tool` value across a registry save.
  - ⛔ The Codex stage's downgrade warning applies here too. A build without the `tool` field treats an Antigravity profile as Claude. Remove the profile or restore `registry.json` first.
  - Measured on Antigravity CLI 1.3.0: a logged-in `agy models` exits 0, and a logged-out one exits 1. The token is a file under the profile's `.gemini`, measured on Linux under WSL only.
  - Antigravity runs a one-time migration on any start that finds no `config/.migrated` marker in `.gemini`. It moves the old-location skills directory and MCP configuration file into `config/`, and the file move replaces a `config/` file that is already there. The seed copies the marker, so the copies stay as they are in the real HOME.
  - A whole Windows build was not checked on this stage's machine. The non-Unix path is one helper, compiled and tested on every platform.
- **Lessons (three fix rounds and a live check):**
  - Seed by mirroring the source, including the marker that says a migration has already run. A copy without it was rewritten by the tool's own first start, and one configuration file replaced another.
  - Survey a tool's directory one level deeper than the brief seems to need. The allowlist missed a skills directory because the survey stopped at the top level.
  - Two byte-identical cache directories don't show that two installs load the same things. One side was weeks stale. Read the running processes.
  - A login verdict belongs to what the login leaves behind (the token, and a status command), never to the interactive tool's exit code.
  - A `#[cfg(not(unix))]` item is invisible to every gate on a Unix machine. Make its body one call to a helper that is compiled everywhere, and test the helper.
  - A dialog that lists what will be deleted needs a test with a realistic number of long names, at the smallest supported terminal size.
  - A prompt loop must end on end-of-file. One spun a core and wrote gigabytes of prompts when its input was closed.
- **Next:** Phase 4, a status line, which needs an incremental path faster than rewriting the whole ledger.

## 📚 Detailed history

⚠️ **This repository is PUBLIC, so the full internal history is deliberately NOT kept here.** This file carries the sanitized technical state only.

The complete record lives in the private `ai-synthesizer` workspace at `knowledge/projects/_memory/project_claude_switch_fork.md` — session-by-session, including the parts that must not be published (hosting account details, client agreements, internal IDs). Folded there 2026-08-17.
