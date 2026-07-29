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
