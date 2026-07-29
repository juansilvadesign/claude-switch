# claude-switch Fork — Build Task Tracker

> **Open this first, every session.** Active implementation work lives here. Check a box when it is complete and leave a short note when it is only partial. Deferred/v2+ ideas live in [`ROADMAP.md`](ROADMAP.md); user-facing behavior and installation live in [`README.md`](README.md).

**Current objective:** make the TUI's account-creation flow unambiguous and prevent a user who wants a different Claude account from accidentally cloning the default account's credentials.

**Plan status:** Phase 1 implemented and verified 2026-07-29 (78 tests, clippy clean). **The real second-account login landed 2026-07-29** — `business` now holds `admin@zoku.com.br`, a genuinely different account from `personal`. Phase 1 is closed.

**Keep these concepts separate:**

| Concept | Meaning | Source of truth |
|---|---|---|
| **Profile name** | Local label such as `work` or `personal` | User input |
| **Account email** | Claude account authenticated inside that profile | `claude auth status --json`, with config metadata only as fallback |
| **Copy current session** | Create another isolated config profile for the account currently logged in under `~/.claude` | `ProfileManager::add_profile` |
| **Login to another account** | Seed a clean profile, remove the old identity, and run Claude's OAuth login | `ProfileManager::login_profile` |

**Important:** the fix is **not** an editable email field. Typing a different email into `registry.json` would only relabel the copied credentials; it would not authenticate the profile as that account.

---

## ✅ Done so far (fork state, 2026-07-29)

- [x] **Fork baseline retained.** The project remains a Rust CLI/TUI that isolates Claude Code accounts with one `CLAUDE_CONFIG_DIR` per profile.
- [x] **Warm login flow implemented on `fix/warm-login`.** New-account login seeds settings, skills, and project trust before authentication instead of starting from an empty profile.
- [x] **Old identity is stripped before login.** Credentials and account-scoped keys are removed from the seeded profile so Claude must authenticate again.
- [x] **Purpose-built authentication is used.** `login_profile` launches `claude auth login`, then asks `claude auth status --json` which account was authenticated.
- [x] **Conversation privacy is the default.** Transcripts, prompt history, and machine-local caches are skipped; `--include-history` is an explicit CLI opt-in.
- [x] **CLI `cswitch add <name>` already presents the correct decision.** When an active account exists, it asks whether to copy the current session or log in to a different account.
- [x] **First-run TUI already presents both choices.** The gap is specifically the normal TUI after at least one profile exists.
- [x] **36 profile/storage unit tests exist.** They cover email extraction, registry operations, filtered copying, identity sanitization, overwrite behavior, aliases, and history isolation.

## 🔍 Root cause, measured 2026-07-29

The user reported being unable to log in as a second account. Three independent causes, all confirmed against the live machine:

1. **No login had ever run.** `~/.claude/.credentials.json`, `profiles/personal/.credentials.json` and `profiles/business/.credentials.json` were **byte-identical** (`sha256 f5d03d01…`, 1004 bytes), and both profiles still carried an unsanitized `oauthAccount` plus the same `userID`. `seed_profile_dir` deletes the credential file before authenticating, so neither profile had been through `login_profile` — both were copies made by `a` / `cswitch add`.
2. **`a` could never yield a different account.** It called `add_profile` → `copy_and_register`, which copies `~/.claude` including credentials. The email was therefore always the default account's, by construction.
3. **`l` could not recover.** `login_profile` bails on an existing non-empty dir, and `handle_login_name` propagated that error with `?` — so pressing `l` and typing an already-taken name tore the TUI down with an error instead of reporting it.

**Fourth cause, outside this codebase:** `claude auth login` delegates account selection to the browser's claude.ai session. A signed-in browser authorises that account with no picker, so even a correctly sanitized profile can come back as the account you already had. Discovered alongside it: `claude auth login --email <addr>` pre-fills the login page (a hint, not an override).

## ✅ Second account, measured 2026-07-29

The login the fix was built for finally ran. Every symptom in the root-cause section is gone:

| | `~/.claude` | `personal` | `business` |
|---|---|---|---|
| `.credentials.json` sha256 | `f5d03d01…` | `f5d03d01…` | **`91ca9db6…`** |
| size | 1004 B | 1004 B | **930 B** |
| `claude auth status` email | — | jaypy.uxdesign@gmail.com | **admin@zoku.com.br** |
| orgId | — | `da41499e…` | **`ebfe92b1…`** |

The byte-identical credential file that defined the original bug is broken apart, the registry email matches live auth for both profiles, and the two accounts sit in separate orgs. `~/.claude` is still `f5d03d01…` — untouched, as the isolation guarantee requires.

## 🔀 Profiles run concurrently — verified 2026-07-29

Isolation is per-process, not per-machine: `CLAUDE_CONFIG_DIR` is read at launch, so several profiles run **at the same time** in different terminals. Confirmed by process table with three distinct config dirs live at once — `business`, `personal` (via `cswitch use personal`), and the default `~/.claude`.

This was never a design goal; it falls out of the mechanism. But it changes the threat model, because a destructive operation now has a target that may be *in use by someone else's terminal*:

- **Refresh and delete could hit a live session.** Addressed in 2.8.
- **`last_used` is a single field.** Concurrent launches make it last-writer-wins, so it records the most recent launch rather than a session. Left alone — see ROADMAP R13.

---

## Phase 1 — P0: Fix “Add account” in the normal TUI

### Problem statement

User report:

> When I use `cswitch` to open the TUI and try to add a new account, I only have the option to add the account name. The email is the same as the default account.

Current behavior in `src/tui.rs`:

1. Pressing `a` enters `Mode::AddName`.
2. The popup asks only for a profile name.
3. Enter calls `ProfileManager::add_profile`.
4. `add_profile` copies the active `~/.claude` session, including its credentials.
5. The new profile therefore has the default account's email.

There is a separate `l` shortcut for `login_profile`, but the word **Add** reasonably leads users to `a` when they want to add an account. The normal TUI also differs from both `cswitch add <name>` and the first-run screen, which already offer copy-versus-login choices.

### Target flow

```text
Press a
  → enter local profile name
  → choose:
      [c] Copy current session (current-email@example.com)
      [l] Login to a different Claude account
  → perform selected operation
  → verify and display the resulting account email
  → return to refreshed profile list
```

- [x] **1.1 Add an explicit TUI state for the operation choice.** `Mode::AddChoice`, separate from `Mode::AddName`. Esc steps back one state and keeps the typed name.
- [x] **1.2 Make `a` mean “Add account/profile,” not silently “copy.”** `handle_add_name` now advances to the choice screen and creates nothing on its own.
- [x] **1.3 Keep `l` as the fast path for experienced users.** Still goes straight to name entry, and queues the same `PendingAction::Login`.
- [x] **1.4 Show the current identity before copying.** The choice screen renders the live email next to Copy, falling back to `unknown account`.
- [x] **1.5 Reuse the existing backend operations.** `c` → `add_profile`, `l` → `login_profile`. No credential logic in `tui.rs`.
- [x] **1.6 Never accept account email as profile metadata input.** `LoginOutcome.email` comes from `claude auth status --json`; `--email` only pre-fills the login page.
- [x] **1.7 Detect same-account results.** `profiles_with_email` feeds `LoginOutcome.same_account_as`; both the TUI and CLI name the matching profiles.
- [x] **1.8 Preserve same-email profiles when intentional.** Reported as a note, not an error — the profile is still registered.
- [x] **1.9 Handle cancellation and auth failure safely.** `abort_login` removes only a directory that attempt created; a failed login registers nothing. An exit code of 0 with no session is also treated as failure.
- [x] **1.10 Resume the TUI after success.** `PendingAction` + `run_pending` tear the terminal down, run the login, then rebuild via `ratatui::init()` and reselect the new profile.

### Acceptance criteria

- [x] From a TUI with an existing profile, pressing `a` visibly offers **Copy current session** and **Login to a different account**.
- [x] Choosing Copy creates a profile with the current account and names that account before it happens.
- [x] Choosing Login opens Claude authentication inside the new profile's `CLAUDE_CONFIG_DIR`.
- [x] Logging in as another account records and displays that account's actual email. — **verified end-to-end 2026-07-29**, see "Second account, measured" below.
- [x] Logging in as the default account displays a same-account warning instead of silently looking like a distinct account.
- [x] No code path changes a profile's email without corresponding authenticated credentials.
- [x] Escape works from name entry and operation choice without creating a profile.
- [x] A failed/cancelled login leaves the registry unchanged and the original `~/.claude` untouched.
- [x] Existing `l`, `r`, search, launch, delete, and first-run behaviors still work.

## Phase 2 — Tests and account-safety hardening

- [x] **2.1 Extract testable TUI transitions.** Key handlers are driven directly in `tui::tests`; `ProfileManager::with_base_dir` isolates the registry and `account_probe` stubs the live-account lookup. No terminal, no OAuth.
- [x] **2.2 Test `a` → name → choice transitions.** Copy, Login, Back, Escape, `q`, empty name, invalid characters, duplicate name.
- [x] **2.3 Test action routing with an injected/stubbed account service.** `choice_l_routes_to_login_and_creates_nothing_yet` and `choice_c_never_routes_to_login` assert the two operations cannot swap.
- [x] **2.4 Test identity reporting.** `profiles_with_email` covered for match, multi-match, case/padding, no match, and unknown-email profiles never matching each other.
- [x] **2.5 Test failed login cleanup.** `login_profile_failure_leaves_other_profiles_byte_for_byte_intact` compares credential bytes and the registry string before/after.
- [x] **2.6 Guard destructive refresh.** `r` now routes through `Mode::ConfirmRefresh`, which shows *holds → will become* and turns red when the accounts differ. Done here rather than deferred: one keystroke could otherwise wipe the business login this work exists to create.
- [x] **2.7 Keep credential values out of UI and logs.** Messages carry name/email/status only; no token is ever formatted.
- [x] **2.8 Warn before overwriting or deleting a profile another session is using.** `ProfileManager::maybe_in_use` reports how recently a Claude session wrote to a profile; `ConfirmRefresh` and `ConfirmDelete` render it and turn red. Advisory, not a block — mtime is evidence, not proof, and a user who knows the other terminal is closed must still be able to proceed.

## Phase 3 — Documentation and release verification

- [x] **3.1 Update TUI keybinding copy** in `README.md`, the footer, and the help popup. `a` now reads "add account", `r` says it overwrites.
- [x] **3.2 Document Copy vs Login** — README "Copy vs Login" section, one example each, plus the browser-session caveat.
- [x] **3.3 Document duplicate-email behavior.** README "Two profiles, one account".
- [x] **3.4 Run `cargo fmt --check`.** **Clean.** Deferred until the behavior changes were committed, then run on its own so the 67 inherited hunks could not bury them — 44 of those were upstream's, present before any of this work. `cargo fmt` touched `main.rs`, `profile.rs`, and `tui.rs`; tests and clippy re-verified green afterwards. Belongs in its own commit, separate from `CHANGELOG.md`.
- [x] **3.5 Run `cargo clippy --all-targets --all-features -- -D warnings`.** Clean. Required fixing 8 pre-existing lints (7 `collapsible_if` → edition-2024 let-chains, 1 `print_literal`) that upstream had left failing.
- [x] **3.6 Run `cargo test`.** **78 passed, 0 failed** — the original 36, plus 25 from Phase 1–2, plus 12 for the live-session guard, plus 5 for symlinked config content.
- [x] **3.7 Manual smoke: Linux.** Different-account Login verified end-to-end 2026-07-29 (see "Second account, measured"). Copy, cancellation, same-account reporting, and relaunch remain covered by tests.
- [ ] **3.8 Cross-platform smoke before release.** Verify macOS Keychain and Windows Credential Manager behavior on their native platforms. **Deliberately left open for the upstream PR** (decision 2026-07-29) — this fork is developed on Linux only, and reviewers on macOS and Windows can exercise the platform paths far better than a simulation here could. Not a gap to close before opening the PR; it is *why* the PR is the right place to close it. What needs exercising:
  - macOS Keychain and Windows Credential Manager credential handling (the original scope).
  - `symlink_to`'s Windows fallback. Windows refuses to create symlinks without Developer Mode or elevation, so it copies the target instead. Never run on real Windows.
  - The live-session guard is pure `fs::metadata` and needs no platform-specific work, but whether its *warning* reads usefully on Windows is untested.
- [x] **3.9 Add a changelog/release note** describing the behavior change without implying that profile names select Claude identities. `CHANGELOG.md`, Keep a Changelog format, `[Unreleased]`. Opens by stating that a profile is a config environment and its name is a local label, then frames each entry as a consequence of that. The browser-session caveat, the read-only email, the project-vs-user skills boundary, and the non-transferring MCP grants are recorded under Notes so they are not rediscovered as bugs.

---

## Cross-cutting checklist

- [x] `~/.claude` is read/copied only; it is never modified by profile creation. Re-verified by hash after removing `business`: `f5d03d01…` unchanged.
- [x] Credentials stay inside the intended profile directory or native credential store.
- [x] Account identity comes from successful Claude authentication, not user-entered metadata.
- [x] Profile names are validated before being used as directory names — the input filter accepts only alphanumerics, `-` and `_`.
- [x] No failed operation overwrites or removes a pre-existing profile.
- [x] Conversation history remains excluded unless the user explicitly opts in.
- [x] TUI labels describe the actual side effect before it happens.
- [x] Platform-specific paths and credential behavior remain behind existing abstractions.

## Decisions recorded

- **2026-07-29 — `a` becomes the unified Add flow.** It will offer Copy or Login, matching the CLI and first-run behavior.
- **2026-07-29 — `l` remains a Login shortcut.** Existing users keep the efficient path.
- **2026-07-29 — email is read-only identity.** There will be no manual account-email field.
- **2026-07-29 — duplicate email warns, not blocks.** Two profiles can intentionally isolate settings for the same account.
- **2026-07-29 — taken names are rejected in the popup, not the backend.** `existing_profile_error` checks before the terminal is torn down, so a duplicate name costs a message rather than the session.
- **2026-07-29 — `login_profile` returns `LoginOutcome`, not `()`.** Callers need the verified email and the same-account list to report honestly; returning unit forced the old code to print from inside the manager.
- **2026-07-29 — a zero exit code is not proof of a session.** `login_profile` re-checks `claude auth status` and treats "exited cleanly, no session" as failure. Otherwise a dismissed browser tab would register a credential-less profile.
- **2026-07-29 — profile `business` deleted.** It held the personal account's credentials (byte-identical to `~/.claude`), so the name was actively misleading. Removed to free it for a real login. `personal` kept.
- **2026-07-29 — the repo is not rustfmt-clean, and that is inherited.** Left as-is so this change stays reviewable; see 3.4. **Resolved once the behavior commits landed** — `cargo fmt` then ran against a clean tree and the churn could not obscure anything.
- **2026-07-29 — live-session detection reads mtimes, not the process table.** Two alternatives were rejected on evidence. A **lockfile written by `cswitch use`** would miss the common case outright: `cswitch aliases` emits `alias claude-x="CLAUDE_CONFIG_DIR=… claude"`, which never runs cswitch, so the guard would report "idle" for the exact sessions it exists to protect — a false negative that costs credentials. A **process-table scan** is `/proc`-specific on Linux, `ps -E` on macOS, and effectively unavailable on Windows. mtime is portable, needs no new dependency, and was measured correct against both live profiles.
- **2026-07-29 — the activity markers are exactly the never-copied ones.** `sessions`, `session-env`, `shell-snapshots` are all in `SEED_SKIP_ALWAYS`, so no copy can stamp them with a current time and make a brand-new profile look occupied. Locked in by `session_markers_can_never_arrive_by_copy`.
- **2026-07-29 — the in-use window is 30 minutes, deliberately generous.** Measured: an actively-typing session rewrites its markers within ~15 s, but an open-and-idle one goes minutes between writes (`personal` measured at 186 s while sitting at a live prompt). A tight window would call that idle. A false positive costs one line in a dialog; a false negative costs an account.
- **2026-07-29 — symlinked config content stays linked.** A link in `~/.claude` is recreated as a link in the profile, not dereferenced into a copy. Linking a skill out of a repository is done *so that* edits propagate; copying would freeze a stale duplicate per profile and quietly break that. The cost is that a profile depends on the source path continuing to exist, which is already true of the original.
- **2026-07-29 — relative link targets are absolutized.** They resolve against the link's own directory, and a profile lives somewhere else, so a verbatim copy would point at nothing — silently, which is the dangerous part. A dangling source link is still reproduced as a dangling link rather than failing the copy: that is the user's existing state, and refusing to create the profile over it would be worse.
- **2026-07-29 — cross-platform verification (3.8) is deferred to the upstream PR, not to a later session here.** This fork is developed on Linux only. Reviewers running macOS and Windows can exercise Keychain, Credential Manager, and the Windows symlink fallback natively, which no amount of local work substitutes for. Recorded as a deliberate choice so a future session does not read the open checkbox as an oversight and try to fake it with mocks.
- **2026-07-29 — the in-use warning advises, it does not block.** mtime cannot distinguish "open in another terminal" from "closed five minutes ago", so `y` keeps its meaning and the user decides. Blocking on a heuristic would make the tool wrong in a way the user cannot override.

## Open questions — resolved 2026-07-29

- [x] **Login return behavior on every terminal.** **Rebuild it.** `run_pending` calls `ratatui::restore()`, runs the login, then assigns a fresh `ratatui::init()` through the `&mut DefaultTerminal` the event loop draws into. Reusing the old instance was not attempted — rebuilding is one line and cannot inherit a corrupted mode.
- [x] **Failed-login directory policy.** **Remove automatically, but only when that attempt created the directory** (`abort_login`'s `we_created_dir` flag). A cancelled login can never delete a directory that already existed, and the registry is untouched either way.
- [x] **Copy wording.** **“Copy current session.”** It names the thing being duplicated rather than the identity, which is the distinction the old wording lost.

## 🧩 What warm state actually transfers — measured 2026-07-29

Prompted by a real report: after switching to `business`, the workspace's own skills were missing while its MCP servers came through. Investigated and **not a `cswitch` bug** — the two kinds of skill live in different places:

| | Where it lives | Follows the profile? |
|---|---|---|
| **User-level skills** | `~/.claude/skills/` | **Yes** — copied by `seed_profile_dir`. All 9 verified present and loaded in the `business` session. |
| **Project-level skills** | `<repo>/.claude/skills/` | **No** — they belong to the repo, not the config dir. `CLAUDE_CONFIG_DIR` does not relocate them and `cswitch` never sees them. |
| **MCP server definitions** | `.claude.json` → `projects` | **Yes** — copied deliberately; the `projects` key is warm state worth keeping. |
| **MCP OAuth grants** | per account, server-side | **No** — a different account has authorised nothing. Whimsical re-prompting is correct behavior, not lost state. |

The workspace's 93 skill symlinks live at `ai-synthesizer/.claude/skills/`. They were absent because the session's project root is the `claude-switch-fork` **git submodule** (`.git` is a gitlink), which stops discovery before that ancestor — the same result under either account. Not profile-related at all.

- [x] **Symlinks in the config directory — found here, fixed.** `copy_dir_all_filtered` tested `is_dir()` before `is_symlink()`. `DirEntry::file_type()` does not follow links on Unix, so a symlink-to-directory reported neither, fell through to `fs::copy`, and aborted the entire profile creation. Reproduced against the real binary before the fix — a linked skill directory gave `the source path is neither a regular file nor a symlink to a regular file`, and a dangling link gave `No such file or directory`. Links are now recreated as links, with relative targets resolved to absolute (a profile lives elsewhere, so `../../repo/x` would silently point at nothing). Verified end-to-end: `cswitch add` succeeds, the linked skill resolves from the profile, edits at the source propagate, and a dangling link stays dangling instead of being fatal.

## 🚀 Upstream PR plan — decided 2026-07-29

**Destination:** propose upstream to `Abhishek21k/claude-switch`. **Split into focused PRs**, not one branch of ten commits. **Code only** — this fork's working documents (`TASKS.md`, `ROADMAP.md`, `CHANGELOG.md`) stay here and are never part of a PR. **`cargo fmt` is not offered**; reformatting 44 hunks of untouched upstream code is unrelated to any fix and only obscures the diff.

### Cherry-picking will not work — author against upstream instead

`upstream/main` has diverged from this fork's internals. Any PR has to be written against upstream's signatures, using this branch as reference rather than as a source of patches:

| | `upstream/main` | this fork |
|---|---|---|
| copy helper | `copy_dir_all(src, dst)` | `copy_dir_all_filtered(src, dst, skip_top_level)` |
| add | `add_profile(name)` | `add_profile(name, include_history)` |
| login | `login_profile(name) -> Result<()>` | `login_profile(name, include_history, email_hint) -> Result<LoginOutcome>` |

`upstream/main` is also 5 months stale locally (`0df4cef`). **Re-fetch before starting** — everything below is measured against that snapshot.

### PR order (each depends on the one above)

1. **Symlink fix — standalone, send first.** Upstream's own `copy_dir_all` has the identical bug: `entry.file_type()?.is_dir()` does not follow links, so a symlink to a directory falls through to `fs::copy` and fails with `Is a directory`, aborting profile creation. **This is reproducible on upstream today and depends on no fork work** — smallest diff, clearest value, best first contact with the maintainer. Port the fix and the five symlink tests onto `copy_dir_all`.
2. **Warm-state seeding** — `seed_profile_dir`, the skip lists, `--include-history`. Introduces the filtered copy, so it builds on PR 1.
3. **Add/Login TUI fix** — `Mode::AddChoice`, `LoginOutcome`, same-account detection. Needs PR 2's `login_profile`.
4. **Live-session guard** — needs `Mode::ConfirmRefresh` from PR 3, so it cannot go earlier.

### Carried into every PR description

- **3.8 as an explicit ask.** State that the work is Linux-only and name what needs a reviewer on another platform: macOS Keychain, Windows Credential Manager, and the Windows symlink fallback.
- **The relevant `CHANGELOG.md` prose**, pasted into the PR body rather than committed — it already states the browser-session caveat and warm-state boundaries a reviewer would otherwise file as bugs.

### Open question — README

"Docs stay fork-only" is unambiguous for the trackers, but `README.md` is user-facing product documentation, not a working document. A PR that changes what `a` does without touching the README's keybinding table ships undocumented behavior, which maintainers usually reject. **Assumption, pending confirmation:** each PR carries only the README lines describing what *that* PR changes; nothing else.

## ▶ Next session — start here

**Phase 3 is closed except 3.8, which is intentionally staying open for the upstream PR.** Nothing here blocks opening it.

1. **`git fetch upstream`.** The local `upstream/main` is 5 months old; the PR plan above is measured against that snapshot and needs re-checking first.
2. **Open PR 1 — the symlink fix** (see the PR plan). Branch off fresh `upstream/main`, port the fix onto upstream's `copy_dir_all`, bring the five symlink tests, and confirm the bug reproduces on unmodified upstream before writing the description.
3. **Confirm the README question** in the PR plan before PR 3, which is the first one that changes user-visible TUI behavior.
4. **Version and release** stays deliberately undecided — `Cargo.toml` is `0.1.0` and `CHANGELOG.md` is `[Unreleased]`. If the work lands upstream, upstream owns the version; the fork only needs its own if the PRs stall or are declined.

**Resolved 2026-07-29 — the two mislabelled commits.** `6603736` and `bb803ed` claimed "cwd-aware profile switching and path mapping", a feature that does not exist here; they actually held the live-session guard and the symlink fix. Rewritten as four commits that say what they contain — guard, symlink fix, `cargo fmt`, docs — and force-pushed. The split was verified by diffing the reconstructed pre-`fmt` tree against the original commit: byte-identical, nothing lost. `backup-before-reword` still points at the old `bb803ed`; delete it once you are satisfied.

## Environment notes

- `cargo` **is** installed at `~/.cargo/bin/cargo` (rustup shim); it is simply absent from non-interactive shells' `PATH`. Prefix with `PATH="$HOME/.cargo/bin:$PATH"`. The earlier "`cargo: command not found`" note was a PATH artifact, not a missing toolchain.
- `rustfmt` and `clippy` components were not installed; added via `rustup component add rustfmt clippy`.
- The built binary is `./target/debug/cswitch`. **`cswitch` is not on `PATH`** — every invocation must use the path, or `cargo install --path .` first. A stale binary silently tests old behavior.
