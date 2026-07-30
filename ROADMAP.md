# claude-switch Fork — Roadmap (post-fix / v2+)

> Deliberately **outside** the active build in [`TASKS.md`](TASKS.md). The current release has one priority: make TUI account creation truthful and safe. These items are retained so that useful follow-ups are not mixed into that fix.

| # | Item | Extends | Why deferred / notes |
|---|---|---|---|
| R1 | **Rename a profile safely** | Registry + profile directories | Renaming touches the registry key, directory path, generated aliases, and any running shell command. Useful, but unrelated to the Add/Login confusion. |
| R2 | **Account health/status check** | `claude auth status --json` | Show logged-in, expired, or needs-reauth state without launching the profile. Defer until the active fix establishes one reusable identity/status abstraction. |
| R3 | **Reauthenticate an existing profile** | Login flow | Provide a safe command for an expired/revoked token without deleting settings or trust state. It needs the same staging and rollback guarantees as Phase 1. |
| R4 | **Identity-safe profile refresh** | TUI `r` / `add_profile_force` | Let users refresh warm config without replacing another account's credentials. The immediate overwrite guard is tracked in `TASKS.md` 2.6; a granular sync model belongs here. |
| R5 | **Choose what warm state to inherit** | `seed_profile_dir` | Advanced switches for settings, skills, MCP/project trust, and conversation history. The current secure defaults should remain the simple path. |
| R6 | **Import an existing config directory** | `add_profile_from` | The tested backend primitive exists, but there is no public CLI/TUI flow. Import needs path validation, identity verification, collision handling, and a clear copy-vs-move contract. |
| R7 | **Profile diagnostics and repair** | Registry/filesystem consistency | Detect missing directories, orphaned directories, unreadable registry data, stale email metadata, and profiles whose live auth identity differs from the registry. |
| R8 | **Shell completions** | clap CLI | Generate bash, zsh, fish, and PowerShell completions for commands and profile names. Helpful after the core account lifecycle is stable. |
| R9 | **Non-interactive/scriptable output** | `list` / `info` | Add JSON output and stable exit codes for launchers and automation. Avoid freezing a machine-readable contract while account status fields are still evolving. |
| R10 | **Cross-platform integration matrix** | CI/release pipeline | Exercise Linux file credentials, macOS Keychain, and Windows Credential Manager with mocked command boundaries plus native smoke jobs. Unit tests alone cannot prove the platform adapters. |
| R11 | **Release automation and provenance** | Cargo + GitHub Releases + Homebrew | Produce checksummed binaries and update distribution metadata from one tagged release. Sequence after the fork's behavior and versioning policy are settled. |
| R12 | **Profile grouping for shared identities** | TUI list/details | When several profiles intentionally use the same email, group or badge them as separate environments for one account. The active fix only needs an honest warning. |
| R13 | **Per-session tracking instead of `last_used`** | Registry | Profiles run concurrently (see `TASKS.md`), so one `last_used` timestamp is last-writer-wins: it records the most recent *launch*, not which profiles are open. A session list would also let the TUI mark live profiles in the list rather than only inside a confirmation. Needs a session lifetime model — start, end, and crash recovery — which is more than the immediate guard required. |
| R14 | **Promote live-session detection from mtime to a real signal** | `maybe_in_use` | The 2.8 guard infers activity from never-copied marker mtimes: dependency-free, but it cannot separate "open in another terminal" from "closed a few minutes ago". **Superseded in practice by R15** — the portability objection recorded here originally was wrong; see that entry. |

## Candidates from `m2selfA/claude-switch` — surveyed 2026-07-30

A [second fork](https://github.com/m2selfA/claude-switch) is 44 commits ahead of upstream and has grown into a multi-provider LLM gateway manager. Most of that is a different product, but several pieces solve problems already listed above. Assessed against this fork's scope, not adopted wholesale.

| # | Item | Extends | Why it is worth taking |
|---|---|---|---|
| R15 | **Process-based session liveness** | `maybe_in_use`, R13, R14 | They track `pid` + `process_started_at` per session and treat *either* "pid not running" *or* "pid start time changed" as stale — the second check catches PID reuse, which a naive liveness test gets wrong. It needs one dependency, `sysinfo`, which covers Linux, macOS and Windows behind a single API. **This corrects R14's stated reasoning:** a process check was rejected as unportable, and that was simply wrong. The argument that survives is narrower — an alias-launched session never runs `cswitch`, so there is no PID to record. So the real design is *both*: definitive for `cswitch use` sessions, mtime as the fallback for alias-launched ones. |
| R16 | **`doctor` command** | R7, parking lot | Their `DiagnosticLevel` / `DiagnosticItem` / `DoctorReport` shape is a good model for the diagnostics R7 wants and the `doctor` command already in the parking lot. Borrow the design, not the code — their implementation is welded to their profile-kind, provider and shim architecture. |
| R17 | **`duplicate <source> <new>`** | R1, R12 | The Copy-vs-Login work established "second environment, same account" as a first-class case. A named command expresses it directly instead of asking users to reason their way to `add` → `[c]`. |
| R18 | **`export` / `import` / `validate`** | R6 | R6 notes the backend primitive (`add_profile_from`) exists with no public flow. This is that flow, and `validate` is the identity-and-collision checking R6 asks for. |
| R19 | **`statusline` / `current`** | Parking lot | "A read-only command showing which profile a terminal is currently using" — worth more now that running several profiles at once is documented behaviour rather than a side effect. |
| R20 | **Separate display name from CLI alias** | Profile naming | Their `add --alias` lets the profile name be arbitrary text while a separate alias stays shell- and path-safe. This fork restricts names to `[A-Za-z0-9_-]` only because the name doubles as a directory name. Small change, removes a real limitation, no architectural risk — probably the cheapest win in this table. |
| R21 | **Lightweight (env-var) profiles alongside full ones** | Profile model | `add <name>` for env-var isolation, `add --full <name>` for directory isolation. Sidesteps warm-state seeding entirely for anyone who only needs to swap credentials. **Think hard before taking this**: it is a second profile model to maintain permanently, and it cuts against this fork's premise that `CLAUDE_CONFIG_DIR` isolation *is* the product. Listed because it is genuinely valuable, not because it is obviously right. |

**Deliberately not taken:** MCP manager, plugin management, provider keys, Paseo, TinyFish, shims and shim recovery, local gateway/runtime modes, SFTP and remote-alias sync. That is a different product. The "Out of scope" section below already excludes administering the remote account; provider-key and gateway management is the same category.

**One observation independent of features:** their test suite is roughly 10,200 lines (`profile/tests.rs` 6,775 + `tui/tests.rs` 3,433) against this fork's 72 tests. Much larger surface, but it is a fair indication of where their confidence comes from.

## Out of scope

- **Manually editing an account email.** Email is authenticated identity, not a profile property the user can safely override.
- **Creating an Anthropic/Claude account.** `cswitch` can launch Claude's authentication flow; account enrollment and recovery remain Claude-owned.
- **Copying credentials between different emails.** A different account must authenticate through Claude.
- **Cloud sync or plaintext export of OAuth credentials.** Any future portability design needs a separate threat model and explicit encryption/recovery decisions.
- **Modifying the default `~/.claude` during profile creation.** The tool's isolation guarantee depends on the default directory remaining untouched.
- **Managing Claude subscriptions, organizations, billing, or permissions.** `cswitch` selects isolated local configurations; it does not administer the remote account.

## Parking lot

- Optional color/icon labels for work, personal, and client profiles.
- Sort by last used, name, or account identity.
- A read-only command showing which profile a terminal is currently using.
- A `doctor` command for Claude binary availability, config permissions, and credential-store access.
- Configurable default profile when launching `cswitch use` without a name.
- TUI mouse support and responsive behavior for very small terminals.
