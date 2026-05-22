use std::fs;
use std::io::Read;
use std::io::Write;
use std::path::Path;
use std::process::Command as ProcessCommand;
use std::thread;
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use chrono::Utc;
use serde::Serialize;

#[derive(Clone, Copy, Debug)]
pub struct AppContext {
    pub dry_run: bool,
    pub debug: bool,
}

#[derive(Clone, Copy, Debug)]
pub enum DeferPeriod {
    Day,
    Week,
    Month,
}

#[derive(Clone, Copy, Debug)]
pub enum Command {
    CheckNewRelease,
    CheckSources,
    DisableThirdParty,
    PreparePackages,
    DryRunUpgrade,
    ScheduleOfflineUpgrade,
    RunAll,
    Defer { period: DeferPeriod },
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LogLevel {
    Debug,
    Info,
    Warn,
    Error,
    Success,
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StepState {
    Pending,
    Running,
    Done,
    Failed,
}

#[derive(Debug, Serialize)]
pub struct Event {
    pub timestamp: String,
    pub level: LogLevel,
    pub step: &'static str,
    pub state: StepState,
    pub message: String,
}

#[derive(Debug, Clone)]
pub struct ReleaseCheckResult {
    pub update_available: bool,
    pub current_major: u32,
    pub current_codename: Option<String>,
    pub stable_major: u32,
    pub stable_codename: String,
    pub testing_codename: Option<String>,
}

const THIRD_PARTY_DIR: &str = "/etc/apt/sources.list.d";
const THIRD_PARTY_STATE_DIR: &str = "/var/lib/debian-upgrade";
const THIRD_PARTY_STATE_FILE: &str = "/var/lib/debian-upgrade/third-party-actions.log";
const DISABLED_LIST_PREFIX: &str = "# debian-upgrade-disabled ";
const DISABLED_SOURCES_MARKER: &str = "# debian-upgrade-disabled-enabled";

// Construit un événement horodaté prêt à être émis côté CLI/GUI.
fn event(level: LogLevel, step: &'static str, state: StepState, message: impl Into<String>) -> Event {
    Event {
        timestamp: Utc::now().to_rfc3339(),
        level,
        step,
        state,
        message: message.into(),
    }
}

// Emet un log debug uniquement si le mode debug est activé.
fn emit_debug(ctx: AppContext, step: &'static str, message: impl Into<String>, emit: &mut dyn FnMut(Event) -> Result<()>) -> Result<()> {
    if ctx.debug {
        emit(event(LogLevel::Debug, step, StepState::Pending, message))?;
    }
    Ok(())
}

// Simule un court temps de travail pour les étapes non branchées en réel.
fn simulate_work() {
    thread::sleep(Duration::from_millis(150));
}

// Exécute une étape générique avec logs communs et mode dry-run.
fn run_step(
    ctx: AppContext,
    step: &'static str,
    description: &str,
    planned_actions: &[&str],
    emit: &mut dyn FnMut(Event) -> Result<()>,
) -> Result<()> {
    emit(event(LogLevel::Info, step, StepState::Running, description))?;

    for action in planned_actions {
        emit_debug(ctx, step, format!("Action prévue: {action}"), emit)?;
    }

    if ctx.dry_run {
        simulate_work();
        emit(event(
            LogLevel::Warn,
            step,
            StepState::Done,
            "Mode dry-run: aucune action système n'a été exécutée.",
        ))?;
    } else {
        simulate_work();
        emit(event(
            LogLevel::Success,
            step,
            StepState::Done,
            "Étape exécutée (implémentation système à brancher).",
        ))?;
    }

    Ok(())
}

// Exécute une commande shell en streamant stdout/stderr vers les événements.
fn run_and_report(
    step: &'static str,
    emit: &mut dyn FnMut(Event) -> Result<()>,
    program: &str,
    args: &[&str],
) -> Result<()> {
    // Stream output progressively to avoid "all logs at once" behavior in GUI.
    let cmd = if args.is_empty() {
        program.to_string()
    } else {
        format!("{program} {}", args.join(" "))
    };
    let mut child = ProcessCommand::new("sh")
        .arg("-c")
        .arg(format!("{cmd} 2>&1"))
        .stdout(std::process::Stdio::piped())
        .spawn()
        .with_context(|| format!("execution {program} {:?}", args))?;

    if let Some(mut stdout) = child.stdout.take() {
        let mut buf = [0u8; 8192];
        let mut acc = String::new();
        loop {
            let n = stdout
                .read(&mut buf)
                .with_context(|| format!("lecture sortie commande: {program}"))?;
            if n == 0 {
                break;
            }
            let chunk = String::from_utf8_lossy(&buf[..n]);
            for ch in chunk.chars() {
                if ch == '\n' || ch == '\r' {
                    let t = acc.trim();
                    if !t.is_empty() {
                        emit(event(LogLevel::Info, step, StepState::Pending, t.to_string()))?;
                    }
                    acc.clear();
                } else {
                    acc.push(ch);
                }
            }
        }
        let t = acc.trim();
        if !t.is_empty() {
            emit(event(LogLevel::Info, step, StepState::Pending, t.to_string()))?;
        }
    }

    let status = child.wait().context("attente fin commande")?;

    if status.success() {
        emit(event(
            LogLevel::Info,
            step,
            StepState::Pending,
            format!("Commande OK: {program} {}", args.join(" ")),
        ))?;
        Ok(())
    } else {
        Err(anyhow!("commande en echec: {program} {}", args.join(" ")))
    }
}

// Lit /etc/os-release et retourne la version majeure + codename local.
fn parse_os_release() -> Result<(u32, Option<String>)> {
    let content = fs::read_to_string("/etc/os-release").context("lecture /etc/os-release")?;
    let mut version_id = None;
    let mut codename = None;

    for line in content.lines() {
        if let Some(v) = line.strip_prefix("VERSION_ID=") {
            version_id = Some(v.trim_matches('"').to_string());
        }
        if let Some(v) = line.strip_prefix("VERSION_CODENAME=") {
            let c = v.trim_matches('"').to_string();
            if !c.is_empty() {
                codename = Some(c);
            }
        }
    }

    let major_str = version_id.ok_or_else(|| anyhow!("VERSION_ID introuvable dans /etc/os-release"))?;
    let major = major_str
        .split('.')
        .next()
        .ok_or_else(|| anyhow!("VERSION_ID invalide: {major_str}"))?
        .parse::<u32>()
        .context("parse major VERSION_ID")?;

    Ok((major, codename))
}

// Détecte si l'APT local expose la sous-commande modernize-sources.
fn apt_supports_modernize_sources() -> bool {
    let output = ProcessCommand::new("apt").arg("--help").output();
    let Ok(output) = output else {
        return false;
    };
    if !output.status.success() {
        return false;
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let combined = format!("{stdout}\n{stderr}");
    combined.contains("modernize-sources")
}

// Réécrit les suites d'un sources.list classique vers le codename cible.
fn rewrite_sources_list_codename(
    file_path: &str,
    from_codename: &str,
    to_codename: &str,
    dry_run: bool,
) -> Result<usize> {
    let content = fs::read_to_string(file_path)
        .with_context(|| format!("lecture {}", file_path))?;
    let mut changed = 0usize;
    let mut rewritten = Vec::new();

    for line in content.lines() {
        let trimmed = line.trim_start();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            rewritten.push(line.to_string());
            continue;
        }

        let (main_part, comment_part) = match line.split_once('#') {
            Some((a, b)) => (a, Some(b)),
            None => (line, None),
        };

        let mut tokens: Vec<String> = main_part
            .split_whitespace()
            .map(ToString::to_string)
            .collect();

        if tokens.len() >= 3 && (tokens[0] == "deb" || tokens[0] == "deb-src") {
            let suite = tokens[2].clone();
            let replaced = if suite == from_codename {
                tokens[2] = to_codename.to_string();
                true
            } else if suite == format!("{from_codename}-updates") {
                tokens[2] = format!("{to_codename}-updates");
                true
            } else if suite == format!("{from_codename}-security") {
                tokens[2] = format!("{to_codename}-security");
                true
            } else if suite == "stable" || suite == "oldstable" {
                tokens[2] = to_codename.to_string();
                true
            } else if suite == "stable-updates" || suite == "oldstable-updates" {
                tokens[2] = format!("{to_codename}-updates");
                true
            } else if suite == "stable-security" || suite == "oldstable-security" {
                tokens[2] = format!("{to_codename}-security");
                true
            } else {
                false
            };

            if replaced {
                changed += 1;
            }
        }

        let mut rebuilt = tokens.join(" ");
        if let Some(comment) = comment_part {
            if !rebuilt.is_empty() {
                rebuilt.push(' ');
            }
            rebuilt.push('#');
            rebuilt.push_str(comment);
        }
        rewritten.push(rebuilt);
    }

    if changed > 0 && !dry_run {
        let mut out = rewritten.join("\n");
        if content.ends_with('\n') {
            out.push('\n');
        }
        fs::write(file_path, out).with_context(|| format!("ecriture {}", file_path))?;
    }

    Ok(changed)
}

// Réécrit les champs Suites d'un fichier debian.sources vers la cible.
fn rewrite_debian_sources_codename(
    file_path: &str,
    from_codename: &str,
    to_codename: &str,
    dry_run: bool,
) -> Result<usize> {
    let content = fs::read_to_string(file_path)
        .with_context(|| format!("lecture {}", file_path))?;
    let mut changed = 0usize;
    let mut rewritten = Vec::new();

    for line in content.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("Suites:") {
            let mut parts = trimmed.split_whitespace();
            let _ = parts.next(); // Suites:
            let mut suites = Vec::new();

            for suite in parts {
                let replaced = if suite == from_codename {
                    changed += 1;
                    to_codename.to_string()
                } else if suite == format!("{from_codename}-updates") {
                    changed += 1;
                    format!("{to_codename}-updates")
                } else if suite == format!("{from_codename}-security") {
                    changed += 1;
                    format!("{to_codename}-security")
                } else if suite == "stable" || suite == "oldstable" {
                    changed += 1;
                    to_codename.to_string()
                } else if suite == "stable-updates" || suite == "oldstable-updates" {
                    changed += 1;
                    format!("{to_codename}-updates")
                } else if suite == "stable-security" || suite == "oldstable-security" {
                    changed += 1;
                    format!("{to_codename}-security")
                } else {
                    suite.to_string()
                };
                suites.push(replaced);
            }

            rewritten.push(format!("Suites: {}", suites.join(" ")));
            continue;
        }

        rewritten.push(line.to_string());
    }

    if changed > 0 && !dry_run {
        let mut out = rewritten.join("\n");
        if content.ends_with('\n') {
            out.push('\n');
        }
        fs::write(file_path, out).with_context(|| format!("ecriture {}", file_path))?;
    }

    Ok(changed)
}

// Sauvegarde sources.list en sources.bak si présent.
fn backup_sources_list_if_present(dry_run: bool) -> Result<bool> {
    let source = "/etc/apt/sources.list";
    let backup = "/etc/apt/sources.bak";
    let source_path = std::path::Path::new(source);
    if !source_path.exists() {
        return Ok(false);
    }
    if dry_run {
        return Ok(true);
    }
    fs::rename(source, backup).with_context(|| format!("rename {} -> {}", source, backup))?;
    Ok(true)
}

// Récupère version/codename de stable et codename de testing depuis Debian.
fn fetch_debian_release_info() -> Result<(u32, String, Option<String>)> {
    let stable_release_url = "https://deb.debian.org/debian/dists/stable/Release";
    let stable_body = reqwest::blocking::get(stable_release_url)
        .and_then(|r| r.error_for_status())
        .context("telechargement Release stable Debian")?
        .text()
        .context("lecture Release stable Debian")?;

    let mut stable_major = None::<u32>;
    let mut stable_codename = None::<String>;
    for line in stable_body.lines() {
        if let Some(v) = line.strip_prefix("Version: ") {
            if let Some(major_str) = v.trim().split('.').next() {
                stable_major = major_str.parse::<u32>().ok();
            }
        }
        if let Some(v) = line.strip_prefix("Codename: ") {
            let c = v.trim();
            if !c.is_empty() {
                stable_codename = Some(c.to_string());
            }
        }
    }

    let stable_major = stable_major.ok_or_else(|| anyhow!("Impossible d'extraire la version Debian stable depuis dists/stable/Release"))?;
    let stable_codename =
        stable_codename.ok_or_else(|| anyhow!("Impossible d'extraire le codename Debian stable depuis dists/stable/Release"))?;

    let testing_release_url = "https://deb.debian.org/debian/dists/testing/Release";
    let testing_codename = reqwest::blocking::get(testing_release_url)
        .and_then(|r| r.error_for_status())
        .ok()
        .and_then(|r| r.text().ok())
        .and_then(|body| {
            body.lines()
                .find_map(|line| line.strip_prefix("Codename: ").map(|v| v.trim().to_string()))
        });

    Ok((stable_major, stable_codename, testing_codename))
}

// Vérifie en ligne s'il existe une nouvelle majeure Debian disponible.
pub fn check_new_major_release(
    ctx: AppContext,
    emit: &mut dyn FnMut(Event) -> Result<()>,
) -> Result<ReleaseCheckResult> {
    let step = "check-new-release";
    emit(event(
        LogLevel::Info,
        step,
        StepState::Running,
        "Verification en ligne de la nouvelle version majeure Debian...",
    ))?;

    let (current_major, current_codename) = parse_os_release()?;
    emit_debug(
        ctx,
        step,
        format!("Systeme local detecte: major={}, codename={:?}", current_major, current_codename),
        emit,
    )?;

    let (stable_major, stable_codename, testing_codename) = fetch_debian_release_info()?;
    emit_debug(
        ctx,
        step,
        format!(
            "Debian releases: stable={} ({}) testing={:?}",
            stable_major, stable_codename, testing_codename
        ),
        emit,
    )?;

    let available = stable_major > current_major;
    if available {
        emit(event(
            LogLevel::Success,
            step,
            StepState::Done,
            format!(
                "Nouvelle version majeure disponible: {} ({})",
                stable_major, stable_codename
            ),
        ))?;
    } else if ctx.debug {
        emit(event(
            LogLevel::Warn,
            step,
            StepState::Done,
            format!(
                "Aucune nouvelle version majeure Debian disponible (stable {} - {}). Mode debug: poursuite autorisee pour tests.",
                stable_major, stable_codename
            ),
        ))?;
    } else {
        emit(event(
            LogLevel::Warn,
            step,
            StepState::Done,
            format!(
                "Aucune nouvelle version majeure Debian disponible. Version stable actuelle: {} ({})",
                stable_major, stable_codename
            ),
        ))?;
    }

    Ok(ReleaseCheckResult {
        update_available: available,
        current_major,
        current_codename,
        stable_major,
        stable_codename,
        testing_codename,
    })
}

// Vérifie et normalise les sources APT avec fallback Debian 12/13.
fn run_check_sources(ctx: AppContext, emit: &mut dyn FnMut(Event) -> Result<()>) -> Result<()> {
    let mut planned_actions = vec![
        "Inspecter /etc/apt/sources.list",
        "Inspecter /etc/apt/sources.list.d/*.list et *.sources",
        "Identifier le codename cible majeur",
        "Remplacer tous les codenames courants par le nouveau codename cible",
    ];

    let supports_modernize = apt_supports_modernize_sources();
    if supports_modernize {
        planned_actions.push("Moderniser les sources officielles: apt modernize-sources");
    } else {
        planned_actions.push(
            "Fallback Debian 12: ignorer apt modernize-sources et normaliser manuellement les fichiers .list/.sources",
        );
    }

    run_step(
        ctx,
        "check-sources",
        "Vérification de la normalisation des sources APT...",
        &planned_actions,
        emit,
    )?;

    if supports_modernize {
        emit(event(
            LogLevel::Info,
            "check-sources",
            StepState::Pending,
            "Compatibilité APT: 'modernize-sources' disponible.",
        ))?;
    } else {
        emit(event(
            LogLevel::Warn,
            "check-sources",
            StepState::Pending,
            "Compatibilité APT: 'modernize-sources' non disponible (cas courant Debian 12), fallback manuel actif.",
        ))?;
    }

    let (current_major, current_codename) = parse_os_release()?;
    if current_major == 12 && !supports_modernize {
        let (_, stable_codename, _) = fetch_debian_release_info()?;
        let source_codename = current_codename.unwrap_or_else(|| "bookworm".to_string());
        let debian_sources_path = "/etc/apt/sources.list.d/debian.sources";
        let debian_sources_exists = std::path::Path::new(debian_sources_path).exists();

        if debian_sources_exists {
            let changed_sources = rewrite_debian_sources_codename(
                debian_sources_path,
                &source_codename,
                &stable_codename,
                ctx.dry_run,
            )?;
            let backed_up = backup_sources_list_if_present(ctx.dry_run)?;

            if ctx.dry_run {
                emit(event(
                    LogLevel::Warn,
                    "check-sources",
                    StepState::Pending,
                    format!(
                        "Debian 12 detectee: dry-run remplacement '{}' -> '{}' dans debian.sources ({} entree(s)); sources.list {} renomme en sources.bak.",
                        source_codename,
                        stable_codename,
                        changed_sources,
                        if backed_up { "serait" } else { "absent, pas de" }
                    ),
                ))?;
            } else {
                emit(event(
                    LogLevel::Success,
                    "check-sources",
                    StepState::Done,
                    format!(
                        "Debian 12 detectee: debian.sources migre '{}' -> '{}' ({} entree(s)); sources.list {} renomme en sources.bak.",
                        source_codename,
                        stable_codename,
                        changed_sources,
                        if backed_up { "a ete" } else { "absent, pas de" }
                    ),
                ))?;
            }
        } else {
            let changed_list = rewrite_sources_list_codename(
                "/etc/apt/sources.list",
                &source_codename,
                &stable_codename,
                ctx.dry_run,
            )?;
            if ctx.dry_run {
                emit(event(
                    LogLevel::Warn,
                    "check-sources",
                    StepState::Pending,
                    format!(
                        "Debian 12 detectee: dry-run remplacement '{}' -> '{}' dans /etc/apt/sources.list ({} entree(s) ciblee(s)).",
                        source_codename, stable_codename, changed_list
                    ),
                ))?;
            } else {
                emit(event(
                    LogLevel::Success,
                    "check-sources",
                    StepState::Done,
                    format!(
                        "Debian 12 detectee: /etc/apt/sources.list migre '{}' -> '{}' ({} entree(s) modifiee(s)).",
                        source_codename, stable_codename, changed_list
                    ),
                ))?;
            }
        }
    }

    Ok(())
}

// Désactive les dépôts tiers pour sécuriser la montée de version.
fn disable_list_repo_lines(
    file_path: &std::path::Path,
    dry_run: bool,
    actions: &mut Vec<String>,
) -> Result<(usize, usize)> {
    let content = fs::read_to_string(file_path)
        .with_context(|| format!("lecture {}", file_path.display()))?;
    let mut changed = 0usize;
    let mut already_disabled = 0usize;
    let mut rewritten = Vec::new();

    for line in content.lines() {
        let trimmed = line.trim_start();
        if let Some(rest) = trimmed.strip_prefix('#') {
            let inner = rest.trim_start();
            if inner.starts_with("deb ") || inner == "deb" || inner.starts_with("deb-src ") || inner == "deb-src" {
                already_disabled += 1;
            }
        }
        if trimmed.starts_with('#') {
            rewritten.push(line.to_string());
            continue;
        }

        if trimmed.starts_with("deb ") || trimmed == "deb" || trimmed.starts_with("deb-src ") || trimmed == "deb-src" {
            rewritten.push(format!("{DISABLED_LIST_PREFIX}{line}"));
            actions.push(format!("list-comment|{}|{}", file_path.display(), line.trim()));
            changed += 1;
        } else {
            rewritten.push(line.to_string());
        }
    }

    if changed > 0 && !dry_run {
        let mut out = rewritten.join("\n");
        if content.ends_with('\n') {
            out.push('\n');
        }
        fs::write(file_path, out).with_context(|| format!("ecriture {}", file_path.display()))?;
    }

    Ok((changed, already_disabled))
}

// Désactive chaque entrée deb822 d'un fichier .sources via "Enabled: no".
fn disable_sources_entries(
    file_path: &std::path::Path,
    dry_run: bool,
    actions: &mut Vec<String>,
) -> Result<(usize, usize)> {
    let content = fs::read_to_string(file_path)
        .with_context(|| format!("lecture {}", file_path.display()))?;
    let mut lines: Vec<String> = content.lines().map(ToString::to_string).collect();
    let mut changed = 0usize;
    let mut already_disabled = 0usize;
    let mut i = 0usize;

    while i < lines.len() {
        while i < lines.len() && lines[i].trim().is_empty() {
            i += 1;
        }
        if i >= lines.len() {
            break;
        }

        let stanza_start = i;
        while i < lines.len() && !lines[i].trim().is_empty() {
            i += 1;
        }
        let stanza_end = i;

        let mut enabled_idx = None::<usize>;
        let mut enabled_is_no = false;
        for (idx, line) in lines[stanza_start..stanza_end].iter().enumerate() {
            let trimmed = line.trim_start();
            if trimmed.starts_with('#') {
                continue;
            }
            if let Some((k, v)) = trimmed.split_once(':') {
                if k.trim().eq_ignore_ascii_case("Enabled") {
                    enabled_idx = Some(stanza_start + idx);
                    enabled_is_no = v.trim().eq_ignore_ascii_case("no");
                    break;
                }
            }
        }

        match enabled_idx {
            Some(idx) => {
                if !enabled_is_no {
                    if idx == stanza_start || lines[idx - 1].trim() != DISABLED_SOURCES_MARKER {
                        lines.insert(idx, DISABLED_SOURCES_MARKER.to_string());
                        i += 1;
                    }
                    lines[idx] = "Enabled: no".to_string();
                    actions.push(format!("sources-enabled-no|{}|stanza-start={}", file_path.display(), stanza_start));
                    changed += 1;
                } else {
                    already_disabled += 1;
                }
            }
            None => {
                lines.insert(stanza_end, DISABLED_SOURCES_MARKER.to_string());
                lines.insert(stanza_end + 1, "Enabled: no".to_string());
                actions.push(format!("sources-enabled-added|{}|stanza-start={}", file_path.display(), stanza_start));
                changed += 1;
                i += 2;
            }
        }
    }

    if changed > 0 && !dry_run {
        let mut out = lines.join("\n");
        if content.ends_with('\n') {
            out.push('\n');
        }
        fs::write(file_path, out).with_context(|| format!("ecriture {}", file_path.display()))?;
    }

    Ok((changed, already_disabled))
}

// Persiste un journal root des actions de désactivation/réactivation des dépôts tiers.
fn persist_third_party_actions(lines: &[String]) -> Result<()> {
    let state_dir = Path::new(THIRD_PARTY_STATE_DIR);
    fs::create_dir_all(state_dir).with_context(|| format!("creation {}", state_dir.display()))?;
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(THIRD_PARTY_STATE_FILE)
        .with_context(|| format!("ouverture {}", THIRD_PARTY_STATE_FILE))?;
    for line in lines {
        writeln!(file, "[{}] {}", Utc::now().to_rfc3339(), line)
            .with_context(|| format!("ecriture {}", THIRD_PARTY_STATE_FILE))?;
    }
    Ok(())
}

// Désactive les dépôts tiers pour sécuriser la montée de version.
fn run_disable_third_party(ctx: AppContext, emit: &mut dyn FnMut(Event) -> Result<()>) -> Result<()> {
    let step = "disable-third-party";
    emit(event(
        LogLevel::Info,
        step,
        StepState::Running,
        "Désactivation des sources tierces...",
    ))?;

    let dir = Path::new(THIRD_PARTY_DIR);
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => {
            emit(event(
                LogLevel::Warn,
                step,
                StepState::Done,
                "Aucun repertoire /etc/apt/sources.list.d accessible.",
            ))?;
            return Ok(());
        }
    };

    let mut changed_files = 0usize;
    let mut changed_entries = 0usize;
    let mut already_disabled_entries = 0usize;
    let mut actions = Vec::<String>::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if name == "debian.sources" {
            continue;
        }
        let Some(ext) = path.extension().and_then(|e| e.to_str()) else {
            continue;
        };
        if ext != "list" && ext != "sources" {
            continue;
        }

        let (changed_in_file, already_disabled_in_file) = if ext == "sources" {
            disable_sources_entries(&path, ctx.dry_run, &mut actions)?
        } else {
            disable_list_repo_lines(&path, ctx.dry_run, &mut actions)?
        };
        already_disabled_entries += already_disabled_in_file;

        if changed_in_file == 0 {
            continue;
        }

        changed_files += 1;
        changed_entries += changed_in_file;
        if ctx.dry_run {
            emit(event(
                LogLevel::Warn,
                step,
                StepState::Pending,
                format!(
                    "Dry-run: desactivation tierce prevue dans {} ({} entree(s) modifiee(s)).",
                    path.display(),
                    changed_in_file
                ),
            ))?;
        } else {
            emit(event(
                LogLevel::Info,
                step,
                StepState::Pending,
                format!(
                    "Depot tiers desactive dans {} ({} entree(s) modifiee(s)).",
                    path.display(),
                    changed_in_file
                ),
            ))?;
        }
    }

    if !ctx.dry_run && !actions.is_empty() {
        let mut action_lines = Vec::with_capacity(actions.len() + 1);
        action_lines.push("disable-third-party|start".to_string());
        action_lines.extend(actions);
        action_lines.push(format!(
            "disable-third-party|summary|changed_files={changed_files}|changed_entries={changed_entries}|already_disabled_entries={already_disabled_entries}"
        ));
        persist_third_party_actions(&action_lines)?;
    }

    if ctx.dry_run {
        emit(event(
            LogLevel::Warn,
            step,
            StepState::Done,
            format!(
                "Mode dry-run: {} fichier(s) tiers seraient modifies ({} entree(s) desactivee(s)).",
                changed_files, changed_entries
            ),
        ))
    } else {
        emit(event(
            LogLevel::Success,
            step,
            StepState::Done,
            format!(
                "{} fichier(s) tiers modifies ({} entree(s) desactivee(s)).",
                changed_files, changed_entries
            ),
        ))
    }
}

// Prépare les paquets nécessaires (clean, update, download-only dist-upgrade).
fn run_prepare_packages(ctx: AppContext, emit: &mut dyn FnMut(Event) -> Result<()>) -> Result<()> {
    let step = "prepare-packages";

    emit(event(
        LogLevel::Info,
        step,
        StepState::Running,
        "Nettoyage cache APT et téléchargement des paquets...",
    ))?;

    emit_debug(ctx, step, "Commande: apt-get clean", emit)?;
    emit_debug(ctx, step, "Commande: env DEBIAN_FRONTEND=noninteractive DEBIAN_PRIORITY=critical APT_LISTCHANGES_FRONTEND=none apt-get update", emit)?;
    emit_debug(
        ctx,
        step,
        "Commande: env DEBIAN_FRONTEND=noninteractive DEBIAN_PRIORITY=critical APT_LISTCHANGES_FRONTEND=none apt-get -y -o Dpkg::Options::=--force-confdef -o Dpkg::Options::=--force-confold -o APT::Get::Always-Include-Phased-Updates=true --download-only dist-upgrade",
        emit,
    )?;

    if ctx.dry_run {
        emit(event(
            LogLevel::Warn,
            step,
            StepState::Done,
            "Mode dry-run: aucune action système n'a été exécutée.",
        ))?;
        return Ok(());
    }

    run_and_report(step, emit, "apt-get", &["clean"])?;
    run_and_report(
        step,
        emit,
        "env",
        &[
            "DEBIAN_FRONTEND=noninteractive",
            "DEBIAN_PRIORITY=critical",
            "APT_LISTCHANGES_FRONTEND=none",
            "apt-get",
            "update",
        ],
    )?;
    run_and_report(
        step,
        emit,
        "env",
        &[
            "DEBIAN_FRONTEND=noninteractive",
            "DEBIAN_PRIORITY=critical",
            "APT_LISTCHANGES_FRONTEND=none",
            "apt-get",
            "-y",
            "-o",
            "Dpkg::Options::=--force-confdef",
            "-o",
            "Dpkg::Options::=--force-confold",
            "-o",
            "APT::Get::Always-Include-Phased-Updates=true",
            "--download-only",
            "dist-upgrade",
        ],
    )?;

    emit(event(
        LogLevel::Success,
        step,
        StepState::Done,
        "Preparation des paquets terminee.",
    ))
}

// Lance un test apt-get -s dist-upgrade en mode non interactif.
fn run_dry_run_upgrade(ctx: AppContext, emit: &mut dyn FnMut(Event) -> Result<()>) -> Result<()> {
    let step = "dry-run-upgrade";
    emit(event(
        LogLevel::Info,
        step,
        StepState::Running,
        "Test dry-run de la mise a niveau...",
    ))?;

    emit_debug(
        ctx,
        step,
        "Commande: env DEBIAN_FRONTEND=noninteractive DEBIAN_PRIORITY=critical APT_LISTCHANGES_FRONTEND=none apt-get -s -o Dpkg::Options::=--force-confdef -o Dpkg::Options::=--force-confold dist-upgrade",
        emit,
    )?;

    if ctx.dry_run {
        emit(event(
            LogLevel::Warn,
            step,
            StepState::Done,
            "Mode dry-run: simulation uniquement, pas d'execution apt-get -s.",
        ))?;
        return Ok(());
    }

    run_and_report(
        step,
        emit,
        "env",
        &[
            "DEBIAN_FRONTEND=noninteractive",
            "DEBIAN_PRIORITY=critical",
            "APT_LISTCHANGES_FRONTEND=none",
            "apt-get",
            "-s",
            "-o",
            "Dpkg::Options::=--force-confdef",
            "-o",
            "Dpkg::Options::=--force-confold",
            "dist-upgrade",
        ],
    )?;
    emit(event(
        LogLevel::Success,
        step,
        StepState::Done,
        "Dry-run upgrade valide.",
    ))
}

// Prépare l'intention d'upgrade hors-ligne et journalise la commande cible.
fn run_schedule_offline_upgrade(ctx: AppContext, emit: &mut dyn FnMut(Event) -> Result<()>) -> Result<()> {
    let step = "schedule-offline-upgrade";
    let reboot_cmd = "env DEBIAN_FRONTEND=noninteractive DEBIAN_PRIORITY=critical APT_LISTCHANGES_FRONTEND=none apt-get -y -o Dpkg::Options::=--force-confdef -o Dpkg::Options::=--force-confold -o APT::Get::Always-Include-Phased-Updates=true dist-upgrade".to_string();

    run_step(
        ctx,
        step,
        "Planification de la mise à niveau hors-ligne au reboot...",
        &[
            "Préparer marqueur d'upgrade hors-ligne",
            "Configurer déclenchement au prochain reboot",
            "Journaliser l'intention d'upgrade",
            "Forcer le mode non interactif APT avec choix par défaut",
        ],
        emit,
    )?;

    if ctx.dry_run {
        emit_debug(
            ctx,
            step,
            format!("Post-reboot (simulation): {reboot_cmd}"),
            emit,
        )?;
    } else {
        emit(event(
            LogLevel::Info,
            step,
            StepState::Pending,
            format!(
                "Commande planifiee au reboot: {reboot_cmd}"
            ),
        ))?;
    }

    Ok(())
}

// Journalise la demande de report de notification selon la période choisie.
fn run_defer(ctx: AppContext, period: DeferPeriod, emit: &mut dyn FnMut(Event) -> Result<()>) -> Result<()> {
    let message = match period {
        DeferPeriod::Day => "Notification reportée de 1 jour.",
        DeferPeriod::Week => "Notification reportée de 1 semaine.",
        DeferPeriod::Month => "Notification reportée de 1 mois.",
    };

    emit_debug(ctx, "defer-notification", format!("Période choisie: {period:?}"), emit)?;

    if ctx.dry_run {
        emit(event(
            LogLevel::Warn,
            "defer-notification",
            StepState::Done,
            format!("{message} (dry-run, non persisté)"),
        ))
    } else {
        emit(event(LogLevel::Info, "defer-notification", StepState::Done, message))
    }
}

// Exécute le pipeline complet de préparation de l'upgrade majeure.
fn run_all(ctx: AppContext, emit: &mut dyn FnMut(Event) -> Result<()>) -> Result<()> {
    emit(event(
        LogLevel::Info,
        "run-all",
        StepState::Running,
        "Démarrage du pipeline complet de préparation...",
    ))?;

    let release = check_new_major_release(ctx, emit)?;
    if !release.update_available && !ctx.debug {
        emit(event(
            LogLevel::Warn,
            "run-all",
            StepState::Done,
            "Processus arrêté: aucune nouvelle version majeure Debian disponible.",
        ))?;
        return Ok(());
    } else if !release.update_available && ctx.debug {
        emit(event(
            LogLevel::Warn,
            "run-all",
            StepState::Pending,
            "Bypass debug actif: poursuite du pipeline malgre absence de nouvelle version majeure.",
        ))?;
    }

    run_check_sources(ctx, emit)?;
    run_disable_third_party(ctx, emit)?;
    run_prepare_packages(ctx, emit)?;
    run_schedule_offline_upgrade(ctx, emit)?;

    emit(event(
        LogLevel::Success,
        "run-all",
        StepState::Done,
        "Pipeline de préparation terminé.",
    ))
}

// Emet les métadonnées de contexte en début d'exécution.
pub fn emit_bootstrap(ctx: AppContext, emit: &mut dyn FnMut(Event) -> Result<()>) -> Result<()> {
    emit_debug(
        ctx,
        "bootstrap",
        format!("Contexte: dry_run={}, debug={}", ctx.dry_run, ctx.debug),
        emit,
    )
}

// Route une commande métier vers son exécuteur concret.
pub fn run_command(ctx: AppContext, command: Command, emit: &mut dyn FnMut(Event) -> Result<()>) -> Result<()> {
    match command {
        Command::CheckNewRelease => {
            let _ = check_new_major_release(ctx, emit)?;
            Ok(())
        }
        Command::CheckSources => run_check_sources(ctx, emit),
        Command::DisableThirdParty => run_disable_third_party(ctx, emit),
        Command::PreparePackages => run_prepare_packages(ctx, emit),
        Command::DryRunUpgrade => run_dry_run_upgrade(ctx, emit),
        Command::ScheduleOfflineUpgrade => run_schedule_offline_upgrade(ctx, emit),
        Command::RunAll => run_all(ctx, emit),
        Command::Defer { period } => run_defer(ctx, period, emit),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    // Vérifie que chaque valeur de report produit bien des événements exploitables.
    fn defer_periods_are_supported() {
        let ctx = AppContext {
            dry_run: true,
            debug: true,
        };

        for period in [DeferPeriod::Day, DeferPeriod::Week, DeferPeriod::Month] {
            let mut events = Vec::<Event>::new();
            let mut sink = |evt: Event| -> Result<()> {
                events.push(evt);
                Ok(())
            };

            run_command(ctx, Command::Defer { period }, &mut sink).expect("defer should work");
            assert!(!events.is_empty());
        }
    }
}
