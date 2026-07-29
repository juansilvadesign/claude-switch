# claude-switch Fork — Build Task Tracker

> **Open this first, every session.** Active implementation work lives here. Check a box when it is complete and leave a short note when it is only partial. Deferred/v2+ ideas live in [`ROADMAP.md`](ROADMAP.md); user-facing behavior and installation live in [`README.md`](README.md).

**Current objective:** make the TUI's account-creation flow unambiguous and prevent a user who wants a different Claude account from accidentally cloning the default account's credentials.

**Plan status:** Phase 1 implemented and verified 2026-07-29 (61 tests, clippy clean). Awaiting the real second-account login, which is a browser step only the user can do.

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
- [ ] Logging in as another account records and displays that account's actual email. — **code path verified, real second account not yet logged in** (browser step, user-only).
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

## Phase 3 — Documentation and release verification

- [x] **3.1 Update TUI keybinding copy** in `README.md`, the footer, and the help popup. `a` now reads "add account", `r` says it overwrites.
- [x] **3.2 Document Copy vs Login** — README "Copy vs Login" section, one example each, plus the browser-session caveat.
- [x] **3.3 Document duplicate-email behavior.** README "Two profiles, one account".
- [ ] **3.4 Run `cargo fmt --check`.** ⚠️ **Not clean, and was not clean before this work.** Baseline (upstream `src/`) already produced **44** `Diff in` hunks; the tree now produces **61**. Deliberately not run: `cargo fmt` rewrites whole files, so it would bury the behavior change under 44 hunks of inherited formatting churn. Do it as its own separate commit.
- [x] **3.5 Run `cargo clippy --all-targets --all-features -- -D warnings`.** Clean. Required fixing 8 pre-existing lints (7 `collapsible_if` → edition-2024 let-chains, 1 `print_literal`) that upstream had left failing.
- [x] **3.6 Run `cargo test`.** **61 passed, 0 failed** — the original 36 plus 25 new.
- [ ] **3.7 Manual smoke: Linux.** Copy, cancellation, same-account reporting, and relaunch are covered by tests; **different-account Login is still unverified end-to-end** because it needs a real second account through the browser.
- [ ] **3.8 Cross-platform smoke before release.** Verify macOS Keychain and Windows Credential Manager behavior on their native platforms.
- [ ] **3.9 Add a changelog/release note** describing the behavior change without implying that profile names select Claude identities.

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
- **2026-07-29 — the repo is not rustfmt-clean, and that is inherited.** Left as-is so this change stays reviewable; see 3.4.

## Open questions — resolved 2026-07-29

- [x] **Login return behavior on every terminal.** **Rebuild it.** `run_pending` calls `ratatui::restore()`, runs the login, then assigns a fresh `ratatui::init()` through the `&mut DefaultTerminal` the event loop draws into. Reusing the old instance was not attempted — rebuilding is one line and cannot inherit a corrupted mode.
- [x] **Failed-login directory policy.** **Remove automatically, but only when that attempt created the directory** (`abort_login`'s `we_created_dir` flag). A cancelled login can never delete a directory that already existed, and the registry is untouched either way.
- [x] **Copy wording.** **“Copy current session.”** It names the thing being duplicated rather than the identity, which is the distinction the old wording lost.

## ▶ Next session — start here

1. **Log the business account in** (user-only, needs a browser): sign out of claude.ai or open a private window, then `./target/debug/cswitch login business --email <business address>`.
2. Verify with `CLAUDE_CONFIG_DIR=~/.claude-switch/profiles/business claude auth status --json` that the email really differs from `jaypy.uxdesign@gmail.com`.
3. If it comes back as the personal account again, the browser session won — `cswitch remove business`, sign out properly, retry. The tool now reports this instead of hiding it.
4. Then Phase 3.7–3.9, and decide whether `cargo fmt` gets its own commit.

## Environment notes

- `cargo` **is** installed at `~/.cargo/bin/cargo` (rustup shim); it is simply absent from non-interactive shells' `PATH`. Prefix with `PATH="$HOME/.cargo/bin:$PATH"`. The earlier "`cargo: command not found`" note was a PATH artifact, not a missing toolchain.
- `rustfmt` and `clippy` components were not installed; added via `rustup component add rustfmt clippy`.
- The built binary is `./target/debug/cswitch`. **`cswitch` is not on `PATH`** — every invocation must use the path, or `cargo install --path .` first. A stale binary silently tests old behavior.
