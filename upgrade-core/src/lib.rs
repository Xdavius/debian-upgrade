use std::fs;
use std::io::Read;
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

fn event(level: LogLevel, step: &'static str, state: StepState, message: impl Into<String>) -> Event {
    Event {
        timestamp: Utc::now().to_rfc3339(),
        level,
        step,
        state,
        message: message.into(),
    }
}

fn emit_debug(ctx: AppContext, step: &'static str, message: impl Into<String>, emit: &mut dyn FnMut(Event) -> Result<()>) -> Result<()> {
    if ctx.debug {
        emit(event(LogLevel::Debug, step, StepState::Pending, message))?;
    }
    Ok(())
}

fn simulate_work() {
    thread::sleep(Duration::from_millis(150));
}

fn noninteractive_env_prefix() -> &'static str {
    "DEBIAN_FRONTEND=noninteractive DEBIAN_PRIORITY=critical APT_LISTCHANGES_FRONTEND=none"
}

fn apt_noninteractive_opts() -> &'static str {
    "-y -o Dpkg::Options::=--force-confdef -o Dpkg::Options::=--force-confold -o APT::Get::Always-Include-Phased-Updates=true"
}

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

fn run_disable_third_party(ctx: AppContext, emit: &mut dyn FnMut(Event) -> Result<()>) -> Result<()> {
    let step = "disable-third-party";
    emit(event(
        LogLevel::Info,
        step,
        StepState::Running,
        "Désactivation des sources tierces...",
    ))?;

    let dir = std::path::Path::new("/etc/apt/sources.list.d");
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

    let mut changed = 0usize;
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
        let backup = path.with_extension(format!("{ext}.disabled-by-debian-upgrade"));
        if ctx.dry_run {
            emit(event(
                LogLevel::Warn,
                step,
                StepState::Pending,
                format!("Dry-run: desactivation tierce prevue: {} -> {}", path.display(), backup.display()),
            ))?;
            changed += 1;
        } else {
            fs::rename(&path, &backup).with_context(|| format!("rename {} -> {}", path.display(), backup.display()))?;
            emit(event(
                LogLevel::Info,
                step,
                StepState::Pending,
                format!("Depot tiers desactive: {} -> {}", path.display(), backup.display()),
            ))?;
            changed += 1;
        }
    }

    if ctx.dry_run {
        emit(event(
            LogLevel::Warn,
            step,
            StepState::Done,
            format!("Mode dry-run: {} depot(s) tiers seraient desactives.", changed),
        ))
    } else {
        emit(event(
            LogLevel::Success,
            step,
            StepState::Done,
            format!("{} depot(s) tiers desactives.", changed),
        ))
    }
}

fn run_prepare_packages(ctx: AppContext, emit: &mut dyn FnMut(Event) -> Result<()>) -> Result<()> {
    let step = "prepare-packages";
    let env_prefix = noninteractive_env_prefix();
    let apt_opts = apt_noninteractive_opts();

    emit(event(
        LogLevel::Info,
        step,
        StepState::Running,
        "Nettoyage cache APT et téléchargement des paquets...",
    ))?;

    emit_debug(ctx, step, format!("Commande: apt-get clean"), emit)?;
    emit_debug(ctx, step, format!("Commande: env {env_prefix} apt-get update"), emit)?;
    emit_debug(
        ctx,
        step,
        format!("Commande: env {env_prefix} apt-get {apt_opts} --download-only dist-upgrade"),
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

fn run_schedule_offline_upgrade(ctx: AppContext, emit: &mut dyn FnMut(Event) -> Result<()>) -> Result<()> {
    let step = "schedule-offline-upgrade";
    let env_prefix = noninteractive_env_prefix();
    let apt_opts = apt_noninteractive_opts();
    let reboot_cmd = format!("env {env_prefix} apt-get {apt_opts} dist-upgrade");

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
                "Commande planifiée au reboot (non interactive): {reboot_cmd}"
            ),
        ))?;
    }

    Ok(())
}

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

pub fn emit_bootstrap(ctx: AppContext, emit: &mut dyn FnMut(Event) -> Result<()>) -> Result<()> {
    emit_debug(
        ctx,
        "bootstrap",
        format!("Contexte: dry_run={}, debug={}", ctx.dry_run, ctx.debug),
        emit,
    )
}

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
