use anyhow::{bail, Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

// ── Data types ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Profile {
    pub name: String,
    pub email: Option<String>,
    pub added: DateTime<Utc>,
    pub last_used: Option<DateTime<Utc>>,
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct Registry {
    pub profiles: HashMap<String, Profile>,
}

// ── ProfileManager ────────────────────────────────────────────────────────────

pub struct ProfileManager {
    #[allow(dead_code)]
    pub base_dir: PathBuf,
    pub profiles_dir: PathBuf,
    registry_path: PathBuf,
}

impl ProfileManager {
    pub fn new() -> Result<Self> {
        let home = dirs::home_dir().context("Cannot determine home directory")?;
        let base_dir = home.join(".claude-switch");
        let profiles_dir = base_dir.join("profiles");
        let registry_path = base_dir.join("registry.json");
        fs::create_dir_all(&profiles_dir)?;
        Ok(Self { base_dir, profiles_dir, registry_path })
    }

    // ── Registry I/O ─────────────────────────────────────────────────────────

    pub fn load_registry(&self) -> Result<Registry> {
        if !self.registry_path.exists() {
            return Ok(Registry::default());
        }
        let content = fs::read_to_string(&self.registry_path)?;
        Ok(serde_json::from_str(&content)?)
    }

    fn save_registry(&self, registry: &Registry) -> Result<()> {
        let content = serde_json::to_string_pretty(registry)?;
        fs::write(&self.registry_path, content)?;
        Ok(())
    }

    // ── Public API ────────────────────────────────────────────────────────────

    /// Returns all profiles sorted alphabetically by name.
    pub fn list_profiles(&self) -> Result<Vec<Profile>> {
        let registry = self.load_registry()?;
        let mut profiles: Vec<Profile> = registry.profiles.into_values().collect();
        profiles.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(profiles)
    }

    /// Add a profile from an explicit source directory.
    /// Seeding from a caller-supplied path; exercised by the tests.
    #[allow(dead_code)]
    pub fn add_profile_from(&self, name: &str, src: &Path) -> Result<Profile> {
        self.copy_and_register(name, src, false, false)
    }

    /// Same as `add_profile_from` but overwrites an existing profile.
    /// Kept as the sibling of `add_profile_from`; exercised by the tests.
    #[allow(dead_code)]
    pub fn add_profile_from_force(&self, name: &str, src: &Path) -> Result<Profile> {
        self.copy_and_register(name, src, false, true)
    }

    /// Add the current logged-in session as a named profile.
    /// Copies `~/.claude/` dir, `~/.claude.json` (home root), and on macOS
    /// extracts Keychain credentials into `.credentials.json`.
    pub fn add_profile(&self, name: &str, include_history: bool) -> Result<Profile> {
        let home = dirs::home_dir().context("Cannot determine home directory")?;
        let src = home.join(".claude");
        if !src.exists() {
            bail!("~/.claude does not exist. Is Claude Code installed and logged in?");
        }
        let mut profile = self.copy_and_register(name, &src, include_history, false)?;
        self.copy_extra_credentials(&home, name, &mut profile)?;
        Ok(profile)
    }

    /// Same as `add_profile` but overwrites an existing profile.
    pub fn add_profile_force(&self, name: &str, include_history: bool) -> Result<Profile> {
        let home = dirs::home_dir().context("Cannot determine home directory")?;
        let src = home.join(".claude");
        if !src.exists() {
            bail!("~/.claude does not exist. Is Claude Code installed and logged in?");
        }
        let mut profile = self.copy_and_register(name, &src, include_history, true)?;
        self.copy_extra_credentials(&home, name, &mut profile)?;
        Ok(profile)
    }

    /// Copy the extra files that live outside `~/.claude/`:
    /// 1. `~/.claude.json` (home root — has oauthAccount metadata)
    /// 2. macOS Keychain credentials → `.credentials.json`
    fn copy_extra_credentials(
        &self,
        home: &Path,
        name: &str,
        profile: &mut Profile,
    ) -> Result<()> {
        let dest = self.profile_dir(name);

        // 1. Copy ~/.claude.json from home root (contains oauthAccount w/ email)
        let home_claude_json = home.join(".claude.json");
        if home_claude_json.exists() {
            fs::copy(&home_claude_json, dest.join(".claude.json"))?;
            // Re-read email now that we have the full config
            if profile.email.is_none() {
                profile.email = read_email_from_dir(&dest);
                self.upsert_profile(profile.clone())?;
            }
        }

        // 2. Extract platform-specific credentials if not already present
        if !dest.join(".credentials.json").exists() {
            if let Some(creds) = extract_platform_credentials() {
                fs::write(dest.join(".credentials.json"), creds)?;
            }
        }

        Ok(())
    }

    pub fn remove_profile(&self, name: &str) -> Result<()> {
        let mut registry = self.load_registry()?;
        if !registry.profiles.contains_key(name) {
            bail!("Profile '{}' not found.", name);
        }
        let dest = self.profiles_dir.join(name);
        if dest.exists() {
            fs::remove_dir_all(&dest)?;
        }
        registry.profiles.remove(name);
        self.save_registry(&registry)
    }

    pub fn get_profile(&self, name: &str) -> Result<Profile> {
        let registry = self.load_registry()?;
        registry
            .profiles
            .get(name)
            .cloned()
            .context(format!("Profile '{}' not found.", name))
    }

    pub fn profile_dir(&self, name: &str) -> PathBuf {
        self.profiles_dir.join(name)
    }

    /// Launch `claude` with `CLAUDE_CONFIG_DIR` pointed at the named profile.
    pub fn launch_claude(&self, name: &str, args: &[String]) -> Result<()> {
        let profile_dir = self.profile_dir(name);
        if !profile_dir.exists() {
            bail!(
                "Profile directory for '{}' not found. Re-add it with: cswitch add {}",
                name,
                name
            );
        }
        let mut registry = self.load_registry()?;
        if let Some(p) = registry.profiles.get_mut(name) {
            p.last_used = Some(Utc::now());
        }
        self.save_registry(&registry)?;

        let status = std::process::Command::new("claude")
            .args(args)
            .env("CLAUDE_CONFIG_DIR", &profile_dir)
            .status()
            .context("Failed to launch claude. Is it installed and in your PATH?")?;

        std::process::exit(status.code().unwrap_or(0));
    }

    /// Create a profile for a *different* account, pre-seeded with the current
    /// account's warm state, then launch Claude so the user can log in.
    ///
    /// An empty profile dir is a blank Claude Code: every project MCP server
    /// drops back to "pending approval", per-directory trust is gone, and
    /// skills/settings are missing — because `CLAUDE_CONFIG_DIR` relocates
    /// `.claude.json` too, not just credentials. So the warm parts of
    /// `~/.claude` are copied first (minus transcripts and prompt history, see
    /// [`SEED_SKIP_HISTORY`]), and everything identifying the *old* account is
    /// removed so Claude runs its normal login flow.
    ///
    /// Safety property: the credential file is always removed. Even if a future
    /// Claude version introduces an identity key this code does not know about,
    /// the profile still cannot authenticate as the old account — Claude has to
    /// re-authenticate and overwrites the stale metadata itself.
    pub fn login_profile(&self, name: &str, include_history: bool) -> Result<()> {
        let profile_dir = self.profiles_dir.join(name);
        if profile_dir.exists() {
            let has_files = profile_dir.read_dir()?.next().is_some();
            if has_files {
                bail!("Profile '{}' already exists. Remove it first or pick a different name.", name);
            }
        }
        fs::create_dir_all(&profile_dir)?;
        let seeded = self.seed_profile_dir(&profile_dir, include_history)?;

        if seeded {
            println!("Seeded '{}' from your current setup (settings, skills, project trust).", name);
            println!("Conversation history and session transcripts were not copied.\n");
        }
        println!("Opening your browser — sign in with the account for profile '{}'.\n", name);

        // `claude auth login` is the purpose-built flow: it opens the browser,
        // waits for the OAuth round-trip, and exits. Launching the full TUI
        // instead would leave the user to remember `/exit`, and would trip
        // Claude's nested-session guard when run from inside a Claude session.
        let status = std::process::Command::new("claude")
            .args(["auth", "login"])
            .env("CLAUDE_CONFIG_DIR", &profile_dir)
            .status()
            .context("Failed to launch claude. Is it installed and in your PATH?")?;

        if !status.success() {
            bail!(
                "Login did not complete for profile '{}'. Nothing was registered — \
                 remove the directory and retry with: cswitch login {}",
                name,
                name
            );
        }

        // Ask Claude who it ended up as, rather than re-parsing the config we
        // just sanitized.
        let email = read_account_email(&profile_dir).or_else(|| read_email_from_dir(&profile_dir));
        let profile = Profile {
            name: name.to_string(),
            email: email.clone(),
            added: Utc::now(),
            last_used: Some(Utc::now()),
        };
        self.upsert_profile(profile)?;

        let display_email = email.as_deref().unwrap_or("unknown");
        println!("\nProfile '{}' registered (account: {}).", name, display_email);
        println!("Launch with: cswitch use {}", name);
        Ok(())
    }

    /// Print shell alias/function lines for all managed profiles.
    /// Auto-detects platform: bash/zsh on Unix, PowerShell on Windows.
    pub fn generate_aliases(&self) -> Result<String> {
        let profiles = self.list_profiles()?;
        if profiles.is_empty() {
            return Ok("# No profiles found. Add one with: cswitch add <name>".to_string());
        }

        if cfg!(target_os = "windows") {
            self.generate_powershell_aliases(&profiles)
        } else {
            self.generate_shell_aliases(&profiles)
        }
    }

    fn generate_shell_aliases(&self, profiles: &[Profile]) -> Result<String> {
        let mut lines = vec![
            "# claude-switch aliases — add to ~/.zshrc or ~/.bashrc".to_string(),
            "# Generated by: cswitch aliases".to_string(),
            String::new(),
        ];
        for p in profiles {
            let dir = self.profile_dir(&p.name);
            let comment = p
                .email
                .as_deref()
                .map(|e| format!("  # {}", e))
                .unwrap_or_default();
            lines.push(format!(
                "alias claude-{}=\"CLAUDE_CONFIG_DIR='{}' claude\"{}",
                p.name,
                dir.display(),
                comment
            ));
        }
        Ok(lines.join("\n"))
    }

    fn generate_powershell_aliases(&self, profiles: &[Profile]) -> Result<String> {
        let mut lines = vec![
            "# claude-switch aliases — add to your PowerShell profile".to_string(),
            "# Run: notepad $PROFILE  to edit your profile".to_string(),
            "# Generated by: cswitch aliases".to_string(),
            String::new(),
        ];
        for p in profiles {
            let dir = self.profile_dir(&p.name);
            let comment = p
                .email
                .as_deref()
                .map(|e| format!("  # {}", e))
                .unwrap_or_default();
            lines.push(format!(
                "function claude-{} {{ $env:CLAUDE_CONFIG_DIR='{}'; claude @args }}{}",
                p.name,
                dir.display(),
                comment
            ));
        }
        Ok(lines.join("\n"))
    }

    // ── Private helpers ───────────────────────────────────────────────────────

    /// Copy the warm half of the live `~/.claude` setup into a fresh profile
    /// dir, then strip everything account-specific so Claude prompts for login.
    ///
    /// Returns `false` without copying anything when there is no live
    /// `~/.claude` to seed from — a first-ever login is still a clean
    /// empty-directory login.
    fn seed_profile_dir(&self, profile_dir: &Path, include_history: bool) -> Result<bool> {
        let Some(home) = dirs::home_dir() else {
            return Ok(false);
        };
        let src = home.join(".claude");
        if !src.exists() {
            return Ok(false);
        }

        copy_dir_all_filtered(&src, profile_dir, &seed_skip(include_history))?;

        // Account metadata lives at the home root, not inside ~/.claude.
        let home_claude_json = home.join(".claude.json");
        if home_claude_json.exists() {
            fs::copy(&home_claude_json, profile_dir.join(".claude.json"))?;
        }

        // Force a fresh login: no credentials, no stale identity.
        let creds = profile_dir.join(".credentials.json");
        if creds.exists() {
            fs::remove_file(&creds)?;
        }
        sanitize_claude_json(&profile_dir.join(".claude.json"))?;

        Ok(true)
    }

    /// Copy a source config dir into a named profile and register it.
    ///
    /// `include_history` keeps conversation content (see [`SEED_SKIP_HISTORY`]);
    /// `force` replaces an existing profile instead of erroring.
    fn copy_and_register(
        &self,
        name: &str,
        src: &Path,
        include_history: bool,
        force: bool,
    ) -> Result<Profile> {
        if !src.exists() {
            bail!("Source directory '{}' does not exist.", src.display());
        }
        let dest = self.profiles_dir.join(name);
        if dest.exists() {
            if force {
                fs::remove_dir_all(&dest)?;
            } else {
                bail!("Profile '{}' already exists. Use --force to overwrite.", name);
            }
        }
        copy_dir_all_filtered(src, &dest, &seed_skip(include_history))?;
        let email = read_email_from_dir(&dest);
        let profile =
            Profile { name: name.to_string(), email, added: Utc::now(), last_used: None };
        self.upsert_profile(profile.clone())?;
        Ok(profile)
    }

    fn upsert_profile(&self, profile: Profile) -> Result<()> {
        let mut registry = self.load_registry()?;
        registry.profiles.insert(profile.name.clone(), profile);
        self.save_registry(&registry)
    }
}

// ── First-run detection ───────────────────────────────────────────────────────

/// Account details read from the live `~/.claude` directory.
pub struct DetectedAccount {
    pub email: Option<String>,
    #[allow(dead_code)]
    pub config_dir: std::path::PathBuf,
}

/// Try to read the currently logged-in Claude account.
/// Checks both `~/.claude/` (config dir) and `~/.claude.json` (home root)
/// since on macOS the account metadata lives at the root, not inside the dir.
/// Returns `None` if neither exists.
pub fn detect_current_account() -> Option<DetectedAccount> {
    let home = dirs::home_dir()?;
    let config_dir = home.join(".claude");
    if !config_dir.exists() {
        return None;
    }
    // Try ~/.claude/ first, then fallback to ~/.claude.json at home root
    let email = read_email_from_dir(&config_dir)
        .or_else(|| read_email_from_home_root(&home));
    Some(DetectedAccount { email, config_dir })
}

// ── Free helpers ──────────────────────────────────────────────────────────────

/// Top-level entries inside a Claude config dir that are never copied into a
/// profile.
///
/// Two independent reasons, both load-bearing:
///
/// - **Privacy.** `projects/` holds full session transcripts and `history.jsonl`
///   holds every prompt ever typed. Copying them puts one account's
///   conversations inside another account's profile — the exact separation a
///   profile exists to create.
/// - **Size.** Transcripts and file-history dominate a real config dir (187 MB
///   of 198 MB on the machine this was developed against), and none of it is
///   state Claude needs in order to start warm.
///
/// Note the name collision this list does *not* touch: the `projects` **key**
/// inside `.claude.json` carries per-directory trust and MCP-server approvals
/// and is deliberately preserved. Only the `projects/` **directory** — the
/// transcripts — is skipped.
/// Conversation content. Skipped by default; kept with `--include-history`.
///
/// Separating a profile's sessions is usually the *point* — but a user who
/// relies on `claude --resume` across a switch can opt back in.
const SEED_SKIP_HISTORY: &[&str] = &[
    "projects",      // session transcripts (NOT the .claude.json "projects" key)
    "history.jsonl", // every prompt ever typed
    "file-history",
    "todos",
];

/// Machine-local caches and runtime state. Never copied, under any flag —
/// stale here at best, confusing at worst.
const SEED_SKIP_ALWAYS: &[&str] = &[
    "sessions",
    "session-env",
    "shell-snapshots",
    "paste-cache",
    "tasks",
    "jobs",
    "daemon",
    "daemon.log",
    "backups",
    "telemetry",
    "cache",
    "ide",
    "statsig",
];

/// Build the skip list for a seed operation.
fn seed_skip(include_history: bool) -> Vec<&'static str> {
    let mut skip = SEED_SKIP_ALWAYS.to_vec();
    if !include_history {
        skip.extend_from_slice(SEED_SKIP_HISTORY);
    }
    skip
}

/// Keys stripped from a seeded `.claude.json` when the profile is going to hold
/// a *different* account.
///
/// `projects` is deliberately absent — it holds the per-directory trust and MCP
/// approvals that are the entire reason to seed a profile instead of starting
/// empty.
const IDENTITY_KEYS: &[&str] = &[
    "oauthAccount",             // email, org, account uuid, billing type
    "userID",                   // per-account analytics id; Claude regenerates it
    "orgModelDefaultCache",     // org-scoped
    "penguinModeOrgEnabled",    // org-scoped
    "claudeAiMcpEverConnected", // account-bound connector list
    "cachedUsageUtilization",
    "cachedExtraUsageDisabledReason",
    "modelAccessCache",
    "additionalModelCostsCache",
    "additionalModelOptionsCache",
];

/// Recursive copy that skips a set of **top-level** entry names.
///
/// Pass an empty `skip_top_level` for a plain full copy.
///
/// The filter is intentionally shallow: the skipped names are all
/// top-level entries of a Claude config dir, and matching at every depth would
/// silently drop a user's own directory that happened to share a name (a
/// project literally called `cache/`, for instance).
///
/// `fs::copy` carries the source permission bits across, so `.credentials.json`
/// keeps its `0600` mode rather than landing world-readable.
fn copy_dir_all_filtered(src: &Path, dst: &Path, skip_top_level: &[&str]) -> Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let name = entry.file_name();
        if skip_top_level.iter().any(|s| name == *s) {
            continue;
        }
        let dest_path = dst.join(&name);
        if entry.file_type()?.is_dir() {
            copy_dir_all_filtered(&entry.path(), &dest_path, &[])?;
        } else {
            fs::copy(entry.path(), dest_path)?;
        }
    }
    Ok(())
}

/// Remove the keys that identify a specific Claude account from a profile's
/// `.claude.json`, leaving the warm state (trust, MCP approvals, onboarding)
/// intact. No-op if the file is missing or unparseable.
fn sanitize_claude_json(path: &Path) -> Result<()> {
    let Ok(content) = fs::read_to_string(path) else {
        return Ok(());
    };
    let Ok(mut val) = serde_json::from_str::<serde_json::Value>(&content) else {
        return Ok(());
    };
    if let Some(obj) = val.as_object_mut() {
        for key in IDENTITY_KEYS {
            obj.remove(*key);
        }
    }
    fs::write(path, serde_json::to_string_pretty(&val)?)?;
    Ok(())
}

/// Extract credentials from the platform's native credential store.
/// - macOS: Keychain via `security`
/// - Windows: Credential Manager via PowerShell
/// - Linux: returns None (credentials are file-based in ~/.claude/.credentials.json,
///   already copied by copy_dir_all)
fn extract_platform_credentials() -> Option<String> {
    #[cfg(target_os = "macos")]
    {
        return extract_macos_keychain();
    }
    #[cfg(target_os = "windows")]
    {
        return extract_windows_credentials();
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        None
    }
}

#[cfg(target_os = "macos")]
fn extract_macos_keychain() -> Option<String> {
    let output = std::process::Command::new("security")
        .args(["find-generic-password", "-s", "Claude Code-credentials", "-w"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let creds = String::from_utf8(output.stdout).ok()?.trim().to_string();
    // Validate it's JSON
    serde_json::from_str::<serde_json::Value>(&creds).ok()?;
    Some(creds)
}

#[cfg(target_os = "windows")]
fn extract_windows_credentials() -> Option<String> {
    // Claude Code on Windows stores credentials in Credential Manager.
    // Use PowerShell to extract them.
    let script = r#"
        $cred = Get-StoredCredential -Target "Claude Code-credentials" -ErrorAction SilentlyContinue
        if ($cred) {
            $cred.GetNetworkCredential().Password
        } else {
            # Fallback: try cmdkey-based extraction via generic credentials
            $bytes = [System.Text.Encoding]::Unicode.GetBytes("")
            $vault = New-Object Windows.Security.Credentials.PasswordVault
            try {
                $entry = $vault.Retrieve("Claude Code-credentials", "")
                $entry.RetrievePassword()
                $entry.Password
            } catch { }
        }
    "#;

    let output = std::process::Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }
    let creds = String::from_utf8(output.stdout).ok()?.trim().to_string();
    if creds.is_empty() {
        return None;
    }
    // Validate it's JSON
    serde_json::from_str::<serde_json::Value>(&creds).ok()?;
    Some(creds)
}

/// Ask Claude which account a profile is actually authenticated as.
///
/// Authoritative where the config files are not: it reflects the live
/// credential, not whatever metadata happens to be on disk. Returns `None` if
/// the profile is logged out or the CLI is too old to have `auth status`.
fn read_account_email(profile_dir: &Path) -> Option<String> {
    let output = std::process::Command::new("claude")
        .args(["auth", "status", "--json"])
        .env("CLAUDE_CONFIG_DIR", profile_dir)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let val: serde_json::Value = serde_json::from_slice(&output.stdout).ok()?;
    if val.get("loggedIn").and_then(|v| v.as_bool()) != Some(true) {
        return None;
    }
    val.get("email")
        .and_then(|e| e.as_str())
        .map(|s| s.to_string())
}

/// Read email from `~/.claude.json` at home root (macOS stores account metadata here).
fn read_email_from_home_root(home: &Path) -> Option<String> {
    let path = home.join(".claude.json");
    if let Ok(content) = fs::read_to_string(&path) {
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(&content) {
            if let Some(email) = val
                .get("oauthAccount")
                .and_then(|o| o.get("emailAddress"))
                .and_then(|e| e.as_str())
            {
                return Some(email.to_string());
            }
        }
    }
    None
}

/// Extract the account email from a Claude config directory.
/// Checks `.claude.json` → `oauthAccount.emailAddress`, then
/// `.credentials.json` → `claudeAiOauth.email` as fallback.
fn read_email_from_dir(dir: &Path) -> Option<String> {
    for filename in &[".claude.json", "claude.json"] {
        if let Ok(content) = fs::read_to_string(dir.join(filename)) {
            if let Ok(val) = serde_json::from_str::<serde_json::Value>(&content) {
                if let Some(email) = val
                    .get("oauthAccount")
                    .and_then(|o| o.get("emailAddress"))
                    .and_then(|e| e.as_str())
                {
                    return Some(email.to_string());
                }
            }
        }
    }
    if let Ok(content) = fs::read_to_string(dir.join(".credentials.json")) {
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(&content) {
            if let Some(email) = val
                .get("claudeAiOauth")
                .and_then(|o| o.get("email"))
                .and_then(|e| e.as_str())
            {
                return Some(email.to_string());
            }
        }
    }
    None
}

// ══════════════════════════════════════════════════════════════════════════════
// Tests
// ══════════════════════════════════════════════════════════════════════════════
#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    // ── Test helpers ──────────────────────────────────────────────────────────

    /// Construct a ProfileManager fully isolated inside a temp directory.
    fn make_manager(tmp: &TempDir) -> ProfileManager {
        let base_dir = tmp.path().join(".claude-switch");
        let profiles_dir = base_dir.join("profiles");
        let registry_path = base_dir.join("registry.json");
        fs::create_dir_all(&profiles_dir).unwrap();
        ProfileManager { base_dir, profiles_dir, registry_path }
    }

    /// Populate a fake `~/.claude` directory with the two files Claude Code
    /// actually writes: `.claude.json` and `.credentials.json`.
    fn make_claude_dir(root: &Path, email: &str) -> PathBuf {
        let dir = root.to_path_buf();
        fs::create_dir_all(&dir).unwrap();

        // .claude.json — contains oauthAccount block
        let claude_json = serde_json::json!({
            "oauthAccount": {
                "emailAddress": email,
                "accountUuid": "uuid-0000-test"
            },
            "someOtherConfig": true
        });
        fs::write(
            dir.join(".claude.json"),
            serde_json::to_string_pretty(&claude_json).unwrap(),
        )
        .unwrap();

        // .credentials.json — contains OAuth tokens
        let creds_json = serde_json::json!({
            "claudeAiOauth": {
                "accessToken": "access_tok",
                "refreshToken": "refresh_tok",
                "expiresAt": 9_999_999_999_u64,
                "scopes": ["user:inference"],
                "subscriptionType": "max"
            }
        });
        fs::write(
            dir.join(".credentials.json"),
            serde_json::to_string_pretty(&creds_json).unwrap(),
        )
        .unwrap();

        dir
    }

    /// Same but email is only in `.credentials.json` to test the fallback path.
    fn make_claude_dir_creds_only(root: &Path, email: &str) -> PathBuf {
        let dir = root.to_path_buf();
        fs::create_dir_all(&dir).unwrap();

        let creds_json = serde_json::json!({
            "claudeAiOauth": {
                "accessToken": "tok",
                "email": email
            }
        });
        fs::write(
            dir.join(".credentials.json"),
            serde_json::to_string_pretty(&creds_json).unwrap(),
        )
        .unwrap();

        dir
    }

    // ── read_email_from_dir ───────────────────────────────────────────────────

    #[test]
    fn email_read_from_claude_json() {
        let tmp = TempDir::new().unwrap();
        let dir = make_claude_dir(tmp.path(), "oauth@test.com");
        assert_eq!(read_email_from_dir(&dir), Some("oauth@test.com".into()));
    }

    #[test]
    fn email_fallback_to_credentials_json() {
        let tmp = TempDir::new().unwrap();
        let dir = make_claude_dir_creds_only(tmp.path(), "creds@test.com");
        assert_eq!(read_email_from_dir(&dir), Some("creds@test.com".into()));
    }

    #[test]
    fn email_returns_none_when_no_config_files() {
        let tmp = TempDir::new().unwrap();
        fs::create_dir_all(tmp.path()).unwrap();
        assert_eq!(read_email_from_dir(tmp.path()), None);
    }

    // ── copy_dir_all ──────────────────────────────────────────────────────────

    #[test]
    fn copy_dir_all_copies_flat_files() {
        let tmp = TempDir::new().unwrap();
        let src = tmp.path().join("src");
        fs::create_dir_all(&src).unwrap();
        fs::write(src.join("a.txt"), "hello").unwrap();
        fs::write(src.join("b.txt"), "world").unwrap();

        let dst = tmp.path().join("dst");
        copy_dir_all_filtered(&src, &dst, &[]).unwrap();

        assert_eq!(fs::read_to_string(dst.join("a.txt")).unwrap(), "hello");
        assert_eq!(fs::read_to_string(dst.join("b.txt")).unwrap(), "world");
    }

    #[test]
    fn copy_dir_all_copies_nested_directories() {
        let tmp = TempDir::new().unwrap();
        let src = tmp.path().join("src");
        fs::create_dir_all(src.join("sub/deep")).unwrap();
        fs::write(src.join("root.txt"), "root").unwrap();
        fs::write(src.join("sub").join("mid.txt"), "mid").unwrap();
        fs::write(src.join("sub/deep").join("leaf.txt"), "leaf").unwrap();

        let dst = tmp.path().join("dst");
        copy_dir_all_filtered(&src, &dst, &[]).unwrap();

        assert_eq!(fs::read_to_string(dst.join("root.txt")).unwrap(), "root");
        assert_eq!(fs::read_to_string(dst.join("sub/mid.txt")).unwrap(), "mid");
        assert_eq!(fs::read_to_string(dst.join("sub/deep/leaf.txt")).unwrap(), "leaf");
    }

    // ── Registry I/O ──────────────────────────────────────────────────────────

    #[test]
    fn load_registry_returns_empty_when_file_absent() {
        let tmp = TempDir::new().unwrap();
        let mgr = make_manager(&tmp);
        let reg = mgr.load_registry().unwrap();
        assert!(reg.profiles.is_empty());
    }

    #[test]
    fn save_and_load_registry_round_trips() {
        let tmp = TempDir::new().unwrap();
        let mgr = make_manager(&tmp);

        let mut reg = Registry::default();
        reg.profiles.insert(
            "work".into(),
            Profile {
                name: "work".into(),
                email: Some("work@acme.com".into()),
                added: Utc::now(),
                last_used: None,
            },
        );
        mgr.save_registry(&reg).unwrap();

        let loaded = mgr.load_registry().unwrap();
        assert_eq!(loaded.profiles.len(), 1);
        assert_eq!(loaded.profiles["work"].email.as_deref(), Some("work@acme.com"));
        assert!(loaded.profiles["work"].last_used.is_none());
    }

    // ── add_profile_from ──────────────────────────────────────────────────────

    #[test]
    fn add_profile_copies_files_into_profiles_dir() {
        let tmp = TempDir::new().unwrap();
        let mgr = make_manager(&tmp);
        let src = make_claude_dir(&tmp.path().join("fake-claude"), "u@test.com");

        mgr.add_profile_from("work", &src).unwrap();

        let dest = mgr.profile_dir("work");
        assert!(dest.join(".claude.json").exists(), ".claude.json missing");
        assert!(dest.join(".credentials.json").exists(), ".credentials.json missing");
    }

    #[test]
    fn add_profile_records_email_from_config() {
        let tmp = TempDir::new().unwrap();
        let mgr = make_manager(&tmp);
        let src = make_claude_dir(&tmp.path().join("fake-claude"), "email@test.com");

        let p = mgr.add_profile_from("personal", &src).unwrap();

        assert_eq!(p.email.as_deref(), Some("email@test.com"));
    }

    #[test]
    fn add_profile_records_entry_in_registry() {
        let tmp = TempDir::new().unwrap();
        let mgr = make_manager(&tmp);
        let src = make_claude_dir(&tmp.path().join("fake-claude"), "x@y.com");

        mgr.add_profile_from("slot", &src).unwrap();

        let reg = mgr.load_registry().unwrap();
        assert!(reg.profiles.contains_key("slot"));
    }

    #[test]
    fn add_profile_stores_none_email_when_config_unreadable() {
        let tmp = TempDir::new().unwrap();
        let mgr = make_manager(&tmp);

        // Source dir exists but contains no recognisable config files
        let src = tmp.path().join("empty-claude");
        fs::create_dir_all(&src).unwrap();
        fs::write(src.join("something-unrelated.txt"), "hi").unwrap();

        let p = mgr.add_profile_from("mystery", &src).unwrap();
        assert!(p.email.is_none());
    }

    #[test]
    fn add_profile_errors_on_nonexistent_source() {
        let tmp = TempDir::new().unwrap();
        let mgr = make_manager(&tmp);
        let err = mgr
            .add_profile_from("bad", &tmp.path().join("does-not-exist"))
            .unwrap_err();
        assert!(err.to_string().contains("does not exist"), "{err}");
    }

    #[test]
    fn add_profile_errors_on_duplicate_without_force() {
        let tmp = TempDir::new().unwrap();
        let mgr = make_manager(&tmp);
        let src = make_claude_dir(&tmp.path().join("fake-claude"), "a@b.com");

        mgr.add_profile_from("dup", &src).unwrap();
        let err = mgr.add_profile_from("dup", &src).unwrap_err();
        assert!(err.to_string().contains("already exists"), "{err}");
    }

    // ── add_profile_from_force ────────────────────────────────────────────────

    #[test]
    fn force_add_overwrites_existing_profile() {
        let tmp = TempDir::new().unwrap();
        let mgr = make_manager(&tmp);

        let src = make_claude_dir(&tmp.path().join("v1"), "first@test.com");
        mgr.add_profile_from("slot", &src).unwrap();

        // Change source to a different account
        let src2 = make_claude_dir(&tmp.path().join("v2"), "second@test.com");
        mgr.add_profile_from_force("slot", &src2).unwrap();

        let reg = mgr.load_registry().unwrap();
        assert_eq!(reg.profiles["slot"].email.as_deref(), Some("second@test.com"));
        // Old files replaced
        let content = fs::read_to_string(mgr.profile_dir("slot").join(".claude.json")).unwrap();
        assert!(content.contains("second@test.com"));
    }

    #[test]
    fn force_add_works_when_profile_does_not_yet_exist() {
        let tmp = TempDir::new().unwrap();
        let mgr = make_manager(&tmp);
        let src = make_claude_dir(&tmp.path().join("fake-claude"), "new@test.com");

        let p = mgr.add_profile_from_force("brand-new", &src).unwrap();
        assert_eq!(p.name, "brand-new");
        assert_eq!(p.email.as_deref(), Some("new@test.com"));
    }

    // ── list_profiles ─────────────────────────────────────────────────────────

    #[test]
    fn list_profiles_returns_empty_vec_when_none_added() {
        let tmp = TempDir::new().unwrap();
        let mgr = make_manager(&tmp);
        assert!(mgr.list_profiles().unwrap().is_empty());
    }

    #[test]
    fn list_profiles_returns_sorted_by_name() {
        let tmp = TempDir::new().unwrap();
        let mgr = make_manager(&tmp);

        for name in &["zebra", "alpha", "mango"] {
            let src = make_claude_dir(
                &tmp.path().join(format!("src-{name}")),
                &format!("{name}@test.com"),
            );
            mgr.add_profile_from(name, &src).unwrap();
        }

        let profiles = mgr.list_profiles().unwrap();
        let names: Vec<&str> = profiles.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["alpha", "mango", "zebra"]);
    }

    // ── remove_profile ────────────────────────────────────────────────────────

    #[test]
    fn remove_profile_deletes_directory_and_registry_entry() {
        let tmp = TempDir::new().unwrap();
        let mgr = make_manager(&tmp);
        let src = make_claude_dir(&tmp.path().join("fake-claude"), "del@test.com");
        mgr.add_profile_from("to-delete", &src).unwrap();

        mgr.remove_profile("to-delete").unwrap();

        assert!(!mgr.profile_dir("to-delete").exists());
        assert!(!mgr.load_registry().unwrap().profiles.contains_key("to-delete"));
    }

    #[test]
    fn remove_profile_errors_when_profile_not_found() {
        let tmp = TempDir::new().unwrap();
        let mgr = make_manager(&tmp);
        let err = mgr.remove_profile("ghost").unwrap_err();
        assert!(err.to_string().contains("not found"), "{err}");
    }

    #[test]
    fn remove_profile_leaves_other_profiles_intact() {
        let tmp = TempDir::new().unwrap();
        let mgr = make_manager(&tmp);

        for name in &["keep", "delete-me"] {
            let src = make_claude_dir(&tmp.path().join(name), &format!("{name}@x.com"));
            mgr.add_profile_from(name, &src).unwrap();
        }

        mgr.remove_profile("delete-me").unwrap();

        let profiles = mgr.list_profiles().unwrap();
        assert_eq!(profiles.len(), 1);
        assert_eq!(profiles[0].name, "keep");
    }

    // ── get_profile ───────────────────────────────────────────────────────────

    #[test]
    fn get_profile_returns_correct_entry() {
        let tmp = TempDir::new().unwrap();
        let mgr = make_manager(&tmp);
        let src = make_claude_dir(&tmp.path().join("fake-claude"), "found@test.com");
        mgr.add_profile_from("found", &src).unwrap();

        let p = mgr.get_profile("found").unwrap();
        assert_eq!(p.name, "found");
        assert_eq!(p.email.as_deref(), Some("found@test.com"));
    }

    #[test]
    fn get_profile_errors_when_missing() {
        let tmp = TempDir::new().unwrap();
        let mgr = make_manager(&tmp);
        let err = mgr.get_profile("nope").unwrap_err();
        assert!(err.to_string().contains("not found"), "{err}");
    }

    // ── profile_dir ───────────────────────────────────────────────────────────

    #[test]
    fn profile_dir_returns_correct_path() {
        let tmp = TempDir::new().unwrap();
        let mgr = make_manager(&tmp);
        assert_eq!(mgr.profile_dir("foo"), mgr.profiles_dir.join("foo"));
    }

    // ── generate_aliases ──────────────────────────────────────────────────────

    #[test]
    fn generate_aliases_when_empty_returns_hint() {
        let tmp = TempDir::new().unwrap();
        let mgr = make_manager(&tmp);
        let out = mgr.generate_aliases().unwrap();
        assert!(out.contains("No profiles"), "{out}");
    }

    #[test]
    fn generate_aliases_includes_all_profiles_with_config_dir() {
        let tmp = TempDir::new().unwrap();
        let mgr = make_manager(&tmp);

        for name in &["work", "personal"] {
            let src = make_claude_dir(&tmp.path().join(name), &format!("{name}@x.com"));
            mgr.add_profile_from(name, &src).unwrap();
        }

        let out = mgr.generate_aliases().unwrap();
        assert!(out.contains("alias claude-work="), "{out}");
        assert!(out.contains("alias claude-personal="), "{out}");
        assert!(out.contains("CLAUDE_CONFIG_DIR="), "{out}");
    }

    // ── login_profile ──────────────────────────────────────────────────────

    #[test]
    fn login_profile_rejects_existing_nonempty_dir() {
        let tmp = TempDir::new().unwrap();
        let mgr = make_manager(&tmp);
        let src = make_claude_dir(&tmp.path().join("fake-claude"), "a@b.com");
        mgr.add_profile_from("taken", &src).unwrap();

        // login_profile should refuse because the dir is non-empty
        let err = mgr.login_profile("taken", false).unwrap_err();
        assert!(err.to_string().contains("already exists"), "{err}");
    }

    // ── read_email_from_home_root ─────────────────────────────────────────

    #[test]
    fn read_email_from_home_root_finds_oauth_account() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        let claude_json = serde_json::json!({
            "oauthAccount": {
                "emailAddress": "root@test.com",
                "accountUuid": "uuid"
            },
            "numStartups": 42
        });
        fs::write(
            root.join(".claude.json"),
            serde_json::to_string_pretty(&claude_json).unwrap(),
        )
        .unwrap();

        assert_eq!(read_email_from_home_root(root), Some("root@test.com".into()));
    }

    #[test]
    fn read_email_from_home_root_returns_none_when_missing() {
        let tmp = TempDir::new().unwrap();
        assert_eq!(read_email_from_home_root(tmp.path()), None);
    }

    #[test]
    fn generate_aliases_includes_email_comment() {
        let tmp = TempDir::new().unwrap();
        let mgr = make_manager(&tmp);
        let src = make_claude_dir(&tmp.path().join("fake"), "me@work.com");
        mgr.add_profile_from("work", &src).unwrap();

        let out = mgr.generate_aliases().unwrap();
        assert!(out.contains("# me@work.com"), "{out}");
    }

    // ── Profile seeding ───────────────────────────────────────────────────────

    #[test]
    fn filtered_copy_skips_named_top_level_entries() {
        let tmp = TempDir::new().unwrap();
        let src = tmp.path().join("src");
        fs::create_dir_all(src.join("projects")).unwrap();
        fs::create_dir_all(src.join("skills")).unwrap();
        fs::write(src.join("projects/transcript.jsonl"), "secret").unwrap();
        fs::write(src.join("history.jsonl"), "every prompt").unwrap();
        fs::write(src.join("settings.json"), "{}").unwrap();
        fs::write(src.join("skills/a.md"), "keep me").unwrap();

        let dst = tmp.path().join("dst");
        copy_dir_all_filtered(&src, &dst, &seed_skip(false)).unwrap();

        assert!(!dst.join("projects").exists(), "transcripts must not be copied");
        assert!(!dst.join("history.jsonl").exists(), "prompt history must not be copied");
        assert!(dst.join("settings.json").exists(), "settings must be copied");
        assert!(dst.join("skills/a.md").exists(), "skills must be copied");
    }

    #[test]
    fn filtered_copy_only_matches_at_top_level() {
        // A user directory that happens to share a skipped name must survive
        // when it is nested — the filter is deliberately shallow.
        let tmp = TempDir::new().unwrap();
        let src = tmp.path().join("src");
        fs::create_dir_all(src.join("skills/cache")).unwrap();
        fs::write(src.join("skills/cache/keep.txt"), "nested").unwrap();

        let dst = tmp.path().join("dst");
        copy_dir_all_filtered(&src, &dst, &seed_skip(false)).unwrap();

        assert!(dst.join("skills/cache/keep.txt").exists());
    }

    #[test]
    fn unfiltered_copy_still_copies_everything() {
        let tmp = TempDir::new().unwrap();
        let src = tmp.path().join("src");
        fs::create_dir_all(src.join("projects")).unwrap();
        fs::write(src.join("projects/t.jsonl"), "x").unwrap();

        let dst = tmp.path().join("dst");
        copy_dir_all_filtered(&src, &dst, &[]).unwrap();

        assert!(dst.join("projects/t.jsonl").exists());
    }

    #[test]
    fn sanitize_strips_identity_but_keeps_trust_state() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join(".claude.json");
        let original = serde_json::json!({
            "oauthAccount": { "emailAddress": "old@work.com" },
            "userID": "abc123",
            "claudeAiMcpEverConnected": ["gmail"],
            "cachedUsageUtilization": { "five_hour": 12 },
            "projects": { "/home/me/repo": { "enabledMcpjsonServers": ["playwright"] } },
            "hasCompletedOnboarding": true
        });
        fs::write(&path, serde_json::to_string_pretty(&original).unwrap()).unwrap();

        sanitize_claude_json(&path).unwrap();

        let out: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        for key in IDENTITY_KEYS {
            assert!(out.get(*key).is_none(), "{key} should have been stripped");
        }
        // The whole point of seeding: trust + MCP approvals survive.
        assert_eq!(
            out["projects"]["/home/me/repo"]["enabledMcpjsonServers"][0],
            "playwright"
        );
        assert_eq!(out["hasCompletedOnboarding"], true);
    }

    #[test]
    fn sanitize_is_a_noop_on_missing_or_invalid_file() {
        let tmp = TempDir::new().unwrap();
        sanitize_claude_json(&tmp.path().join("absent.json")).unwrap();

        let bad = tmp.path().join("bad.json");
        fs::write(&bad, "not json at all").unwrap();
        sanitize_claude_json(&bad).unwrap();
        assert_eq!(fs::read_to_string(&bad).unwrap(), "not json at all");
    }

    #[test]
    fn include_history_keeps_conversations_but_never_machine_state() {
        let tmp = TempDir::new().unwrap();
        let src = tmp.path().join("src");
        fs::create_dir_all(src.join("projects")).unwrap();
        fs::create_dir_all(src.join("shell-snapshots")).unwrap();
        fs::write(src.join("projects/t.jsonl"), "conversation").unwrap();
        fs::write(src.join("history.jsonl"), "prompts").unwrap();
        fs::write(src.join("shell-snapshots/s.sh"), "machine-local").unwrap();

        let dst = tmp.path().join("dst");
        copy_dir_all_filtered(&src, &dst, &seed_skip(true)).unwrap();

        assert!(dst.join("projects/t.jsonl").exists(), "opt-in keeps transcripts");
        assert!(dst.join("history.jsonl").exists(), "opt-in keeps prompt history");
        assert!(
            !dst.join("shell-snapshots").exists(),
            "machine-local state is skipped under every flag"
        );
    }

    #[test]
    fn add_profile_does_not_copy_conversation_history() {
        // Regression guard: `add` clones the *current* account, so it keeps
        // credentials — but transcripts and prompt history are still private
        // and must never land in a profile directory.
        let tmp = TempDir::new().unwrap();
        let mgr = make_manager(&tmp);
        let src = make_claude_dir(&tmp.path().join("fake"), "me@work.com");
        fs::create_dir_all(src.join("projects")).unwrap();
        fs::write(src.join("projects/session.jsonl"), "private conversation").unwrap();
        fs::write(src.join("history.jsonl"), "every prompt").unwrap();

        mgr.add_profile_from("work", &src).unwrap();
        let dest = mgr.profile_dir("work");

        assert!(!dest.join("projects").exists());
        assert!(!dest.join("history.jsonl").exists());
        // ...while the things that make a profile warm are still there.
        assert!(dest.join(".claude.json").exists());
        assert!(dest.join(".credentials.json").exists(), "add keeps the same account logged in");
    }
}
