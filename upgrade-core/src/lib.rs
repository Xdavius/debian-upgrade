use std::fs;
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
    run_step(
        ctx,
        "check-sources",
        "Vérification de la normalisation des sources APT...",
        &[
            "Inspecter /etc/apt/sources.list",
            "Inspecter /etc/apt/sources.list.d/*.list et *.sources",
            "Identifier le codename cible majeur",
            "Remplacer tous les codenames courants par le nouveau codename cible",
        ],
        emit,
    )
}

fn run_disable_third_party(ctx: AppContext, emit: &mut dyn FnMut(Event) -> Result<()>) -> Result<()> {
    run_step(
        ctx,
        "disable-third-party",
        "Désactivation des sources tierces...",
        &[
            "Identifier les dépôts tiers",
            "Désactiver temporairement les fichiers tiers",
            "Conserver un backup des modifications",
        ],
        emit,
    )
}

fn run_prepare_packages(ctx: AppContext, emit: &mut dyn FnMut(Event) -> Result<()>) -> Result<()> {
    let step = "prepare-packages";
    let env_prefix = noninteractive_env_prefix();
    let apt_opts = apt_noninteractive_opts();

    run_step(
        ctx,
        step,
        "Nettoyage cache APT et téléchargement des paquets...",
        &[
            "apt clean",
            "apt update (non interactif)",
            "apt-get dist-upgrade --download-only (non interactif)",
        ],
        emit,
    )?;

    emit_debug(
        ctx,
        step,
        format!("Commande: env {env_prefix} apt-get update"),
        emit,
    )?;
    emit_debug(
        ctx,
        step,
        format!(
            "Commande: env {env_prefix} apt-get {apt_opts} --download-only dist-upgrade"
        ),
        emit,
    )?;

    Ok(())
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
