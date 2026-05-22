use std::thread;
use std::io::{BufRead, BufReader};
use std::{fs, path::Path};
use std::collections::VecDeque;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::sync::OnceLock;
use slint::Model;
use serde::{Deserialize, Serialize};

use upgrade_core::{
    check_new_major_release, run_command, AppContext, Command as CoreCommand, Event as CoreEvent,
    LogLevel, StepState,
};

slint::include_modules!();

struct UiEvent {
    level: String,
    step: String,
    state: String,
    message: String,
}

enum StatusTone {
    Neutral,
    Running,
    Success,
    Warn,
    Error,
}

#[derive(Debug, Serialize, Deserialize, Default)]
struct AppSessionConfig {
    debug_mode: bool,
    selected_third_party_repos: Vec<String>,
}

static DEFERRED_LOG_LINES: OnceLock<Mutex<Vec<String>>> = OnceLock::new();
static LOG_RING: OnceLock<Mutex<VecDeque<String>>> = OnceLock::new();
static PRIVILEGED_AGENT: OnceLock<Mutex<Option<PrivilegedAgentSession>>> = OnceLock::new();
const MAX_LOG_LINES: usize = 1200;
const LOG_FLUSH_INTERVAL_MS: u64 = 40;
const LOG_LINES_PER_FLUSH: usize = 8;
const AGENT_DONE_PREFIX: &str = "__AGENT_DONE__";

// Retourne le buffer temporaire de logs quand la zone de texte est focalisée.
fn deferred_log_lines() -> &'static Mutex<Vec<String>> {
    DEFERRED_LOG_LINES.get_or_init(|| Mutex::new(Vec::new()))
}

// Retourne le ring buffer global utilisé pour borner l'historique des logs.
fn log_ring() -> &'static Mutex<VecDeque<String>> {
    LOG_RING.get_or_init(|| Mutex::new(VecDeque::new()))
}

struct PrivilegedAgentSession {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    debug: bool,
}

fn privileged_agent_slot() -> &'static Mutex<Option<PrivilegedAgentSession>> {
    PRIVILEGED_AGENT.get_or_init(|| Mutex::new(None))
}

#[derive(Clone, Copy)]
struct RunMode {
    debug: bool,
}

// Détecte le mode debug à partir des arguments de lancement GUI.
fn parse_run_mode() -> RunMode {
    let debug = std::env::args().any(|arg| arg == "--debug");
    RunMode { debug }
}

// Retourne le chemin du fichier de session/config utilisateur.
fn session_config_path() -> PathBuf {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .unwrap_or_else(|| PathBuf::from("/tmp"));
    base.join("debian-upgrade").join("session.json")
}

// Charge la config de session, ou crée un fichier par défaut si absent/corrompu.
fn load_or_init_session_config(debug_mode: bool) -> AppSessionConfig {
    let path = session_config_path();
    if let Ok(content) = fs::read_to_string(&path) {
        if let Ok(cfg) = serde_json::from_str::<AppSessionConfig>(&content) {
            return cfg;
        }
    }

    let cfg = AppSessionConfig {
        debug_mode,
        selected_third_party_repos: Vec::new(),
    };
    let _ = save_session_config(&cfg);
    cfg
}

// Sauvegarde la config/session utilisateur sur disque.
fn save_session_config(cfg: &AppSessionConfig) -> anyhow::Result<()> {
    let path = session_config_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let data = serde_json::to_string_pretty(cfg)?;
    fs::write(path, data)?;
    Ok(())
}

// Crée la fenêtre Slint avec fallback de backend de rendu si besoin.
fn create_app_window_with_backend_fallback() -> Result<AppWindow, slint::PlatformError> {
    if std::env::var("SLINT_BACKEND").is_err() {
        std::env::set_var("SLINT_BACKEND", "winit-femtovg");
    }

    match AppWindow::new() {
        Ok(app) => Ok(app),
        Err(err) => {
            if std::env::var("SLINT_BACKEND").ok().as_deref() == Some("winit-femtovg") {
                std::env::set_var("SLINT_BACKEND", "winit-software");
                AppWindow::new()
            } else {
                Err(err)
            }
        }
    }
}

// Helper fallback de saisie mot de passe: zenity puis kdialog.
// L'ordre global d'elevation reste: pkexec d'abord, puis sudo via ce helper.
fn read_password_from_zenity_or_kdialog() -> anyhow::Result<String> {
    let zenity = Command::new("zenity")
        .arg("--password")
        .arg("--title=Authentification requise")
        .output();
    let output = match zenity {
        Ok(o) if o.status.success() => o,
        _ => Command::new("kdialog")
            .arg("--password")
            .arg("Authentification requise")
            .output()
            .map_err(|e| anyhow::anyhow!("zenity/kdialog indisponibles: {e}"))?,
    };

    if !output.status.success() {
        return Err(anyhow::anyhow!("saisie mot de passe annulee ou en echec"));
    }

    let mut password = String::from_utf8(output.stdout)
        .map_err(|e| anyhow::anyhow!("mot de passe invalide UTF-8: {e}"))?;
    while password.ends_with('\n') || password.ends_with('\r') {
        password.pop();
    }
    Ok(password)
}

// Valide un mot de passe sudo sans lancer l'agent, pour éviter tout mélange password/commandes.
fn validate_sudo_password(password: &str) -> anyhow::Result<()> {
    let mut child = Command::new("sudo")
        .arg("-S")
        .arg("-v")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| anyhow::anyhow!("sudo -v impossible a lancer: {e}"))?;
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(format!("{password}\n").as_bytes());
    }
    let status = child
        .wait()
        .map_err(|e| anyhow::anyhow!("sudo -v echec attente: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(anyhow::anyhow!("authentification sudo refusee"))
    }
}

// Résout le chemin du binaire backend (env, voisin, puis chemin système).
fn backend_cli_path() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("DEBIAN_UPGRADE_BACKEND") {
        let pb = PathBuf::from(&p);
        if pb.exists() {
            return Some(pb);
        }
    }

    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let sibling = dir.join("debian-upgrade-backend");
            if sibling.exists() {
                return Some(sibling);
            }
        }
    }

    let system = PathBuf::from("/usr/libexec/debian-upgrade-backend");
    if system.exists() {
        return Some(system);
    }
    None
}

// Parse une ou plusieurs lignes JSON backend en événements UI robustes.
fn parse_backend_json_events(stdout: &[u8]) -> Vec<UiEvent> {
    let mut out = Vec::new();
    let text = String::from_utf8_lossy(stdout);
    for line in text.lines() {
        if line.trim().is_empty() {
            continue;
        }
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(line) {
            let level = v.get("level").and_then(|x| x.as_str()).unwrap_or("info").to_string();
            let step = v.get("step").and_then(|x| x.as_str()).unwrap_or("backend").to_string();
            let state = v.get("state").and_then(|x| x.as_str()).unwrap_or("pending").to_string();
            let message = v.get("message").and_then(|x| x.as_str()).unwrap_or(line).to_string();
            out.push(UiEvent { level, step, state, message });
        } else {
            out.push(UiEvent {
                level: "info".to_string(),
                step: "backend".to_string(),
                state: "pending".to_string(),
                message: line.to_string(),
            });
        }
    }
    out
}

// Exécute des sous-commandes backend avec élévation et streaming d'événements.
fn run_backend_subcommands_via_privileged_backend_stream<F>(
    debug: bool,
    subcommands: &[String],
    mut on_event: F,
) -> anyhow::Result<()>
where
    F: FnMut(UiEvent),
{
    let bin = backend_cli_path().ok_or_else(|| {
        anyhow::anyhow!(
            "backend CLI introuvable. Attendu: voisin du binaire GUI, /usr/libexec/debian-upgrade-backend ou DEBIAN_UPGRADE_BACKEND"
        )
    })?;
    let bin_str = bin.to_string_lossy().to_string();

    let mut slot = privileged_agent_slot()
        .lock()
        .map_err(|_| anyhow::anyhow!("agent mutex lock echec"))?;

    let need_restart = if let Some(agent) = slot.as_mut() {
        if agent.debug != debug {
            true
        } else {
            match agent.child.try_wait() {
                Ok(Some(_)) => true,
                Ok(None) => false,
                Err(_) => true,
            }
        }
    } else {
        true
    };

    if need_restart {
        *slot = Some(start_privileged_agent(&bin_str, debug)?);
    }

    let agent = slot.as_mut().ok_or_else(|| anyhow::anyhow!("agent non initialise"))?;
    for sub in subcommands {
        let sub = sub.as_str();
        agent
            .stdin
            .write_all(format!("{sub}\n").as_bytes())
            .map_err(|e| anyhow::anyhow!("agent write echec ({sub}): {e}"))?;
        agent
            .stdin
            .flush()
            .map_err(|e| anyhow::anyhow!("agent flush echec ({sub}): {e}"))?;

        let mut line = String::new();
        loop {
            line.clear();
            let n = agent
                .stdout
                .read_line(&mut line)
                .map_err(|e| anyhow::anyhow!("agent read echec ({sub}): {e}"))?;
            if n == 0 {
                return Err(anyhow::anyhow!("agent termine de maniere inattendue ({sub})"));
            }
            let l = line.trim();
            if l.is_empty() {
                continue;
            }
            if let Some((prefix, tail)) = l.split_once('|') {
                if prefix == AGENT_DONE_PREFIX {
                    let mut parts = tail.split('|');
                    let status = parts.next().unwrap_or_default();
                    let cmd_done = parts.next().unwrap_or_default();
                    if cmd_done == sub {
                        if status == "ok" {
                            break;
                        }
                        return Err(anyhow::anyhow!("commande agent en echec: {sub}"));
                    }
                }
            }
            for evt in parse_backend_json_events(l.as_bytes()) {
                on_event(evt);
            }
        }
    }
    Ok(())
}

fn start_privileged_agent(bin_str: &str, debug: bool) -> anyhow::Result<PrivilegedAgentSession> {
    let mut args = Vec::new();
    if debug {
        args.push("--dry-run");
        args.push("--debug");
    }
    args.push("agent");

    if let Ok(mut child) = Command::new("pkexec")
        .arg(bin_str)
        .args(&args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
    {
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| anyhow::anyhow!("pkexec agent stdin indisponible"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| anyhow::anyhow!("pkexec agent stdout indisponible"))?;
        return Ok(PrivilegedAgentSession {
            child,
            stdin,
            stdout: BufReader::new(stdout),
            debug,
        });
    }

    let mut child = Command::new("sudo")
        .arg(bin_str)
        .args(&args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| anyhow::anyhow!("sudo agent impossible a lancer: {e}"))?;

    let stdin = child
        .stdin
        .take()
        .ok_or_else(|| anyhow::anyhow!("sudo agent stdin indisponible"))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| anyhow::anyhow!("sudo agent stdout indisponible"))?;
    let mut password = read_password_from_zenity_or_kdialog()?;
    validate_sudo_password(&password)?;
    password.replace_range(.., &"\0".repeat(password.len()));
    password.clear();

    Ok(PrivilegedAgentSession {
        child,
        stdin,
        stdout: BufReader::new(stdout),
        debug,
    })
}

// Ajoute une seule ligne de log dans la vue UI.
fn append_log(app: &AppWindow, line: &str) {
    append_logs_batch(app, &[line.to_string()]);
}

// Ajoute un lot de logs avec buffer différé, ring buffer et auto-scroll contrôlé.
fn append_logs_batch(app: &AppWindow, lines: &[String]) {
    if lines.is_empty() {
        return;
    }

    if app.get_logs_has_focus() {
        if let Ok(mut d) = deferred_log_lines().lock() {
            d.extend(lines.iter().cloned());
        }
        return;
    }

    let mut merged: Vec<String> = Vec::new();
    if let Ok(mut d) = deferred_log_lines().lock() {
        if !d.is_empty() {
            merged.extend(std::mem::take(&mut *d));
        }
    }
    merged.extend(lines.iter().cloned());

    let rendered = if let Ok(mut ring) = log_ring().lock() {
        for line in merged {
            ring.push_back(line);
        }
        while ring.len() > MAX_LOG_LINES {
            ring.pop_front();
        }
        ring.iter().cloned().collect::<Vec<_>>().join("\n")
    } else {
        app.get_logs_text().to_string()
    };

    let rendered_with_padding = if rendered.is_empty() {
        rendered
    } else {
        format!("{rendered}\n\n")
    };
    app.set_logs_text(rendered_with_padding.into());
    if !app.get_logs_has_focus() {
        app.invoke_scroll_logs_to_end(i32::MAX);
    }
}

// Convertit le niveau de log backend vers sa représentation texte UI.
fn level_to_str(level: LogLevel) -> &'static str {
    match level {
        LogLevel::Debug => "debug",
        LogLevel::Info => "info",
        LogLevel::Warn => "warn",
        LogLevel::Error => "error",
        LogLevel::Success => "success",
    }
}

// Convertit l'état d'étape backend vers sa représentation texte UI.
fn state_to_str(state: StepState) -> &'static str {
    match state {
        StepState::Pending => "pending",
        StepState::Running => "running",
        StepState::Done => "done",
        StepState::Failed => "failed",
    }
}

// Transforme un événement coeur backend en événement consommable par la GUI.
fn core_to_ui_event(event: CoreEvent) -> UiEvent {
    UiEvent {
        level: level_to_str(event.level).to_string(),
        step: event.step.to_string(),
        state: state_to_str(event.state).to_string(),
        message: event.message,
    }
}

// Applique un événement UI en l'écrivant dans la zone de logs.
fn apply_ui_event(app: &AppWindow, evt: UiEvent) {
    let tone = if evt.state == "failed" || evt.level == "error" {
        StatusTone::Error
    } else if evt.level == "warn" {
        StatusTone::Warn
    } else if evt.state == "done" && evt.level == "success" {
        StatusTone::Success
    } else if evt.state == "running" {
        StatusTone::Running
    } else {
        StatusTone::Neutral
    };
    set_header_status(app, &evt.message, tone);
    append_log(app, &format!("[{}] {} ({}, {})", evt.level, evt.message, evt.step, evt.state));
}

fn set_header_status(app: &AppWindow, text: &str, tone: StatusTone) {
    app.set_header_status(text.into());
    let color = match tone {
        StatusTone::Neutral => slint::Color::from_rgb_u8(79, 91, 102),
        StatusTone::Running => slint::Color::from_rgb_u8(34, 102, 170),
        StatusTone::Success => slint::Color::from_rgb_u8(32, 128, 74),
        StatusTone::Warn => slint::Color::from_rgb_u8(176, 112, 0),
        StatusTone::Error => slint::Color::from_rgb_u8(182, 46, 46),
    };
    app.set_header_status_color(color);
}

// Planifie un flush périodique des événements tamponnés vers l'UI.
fn schedule_log_flush(
    ui: slint::Weak<AppWindow>,
    pending: Arc<Mutex<Vec<UiEvent>>>,
    scheduled: Arc<AtomicBool>,
) {
    thread::spawn(move || {
        loop {
            thread::sleep(std::time::Duration::from_millis(LOG_FLUSH_INTERVAL_MS));
            let mut lines = Vec::<String>::new();
            if let Ok(mut p) = pending.lock() {
                let n = p.len().min(LOG_LINES_PER_FLUSH);
                lines.reserve(n);
                for _ in 0..n {
                    let evt = p.remove(0);
                    lines.push(format!(
                        "[{}] {} ({}, {})",
                        evt.level, evt.message, evt.step, evt.state
                    ));
                }
            }

            if !lines.is_empty() {
                let ui_apply = ui.clone();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(app) = ui_apply.upgrade() {
                        append_logs_batch(&app, &lines);
                    }
                });
                continue;
            }

            scheduled.store(false, Ordering::SeqCst);
            let has_new = pending.lock().map(|p| !p.is_empty()).unwrap_or(false);
            if has_new
                && scheduled
                    .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
                    .is_ok()
            {
                continue;
            }
            break;
        }
    });
}

// Empile un événement UI dans la file et démarre le scheduler si nécessaire.
fn publish_ui_event_batched(
    ui: &slint::Weak<AppWindow>,
    pending: &Arc<Mutex<Vec<UiEvent>>>,
    scheduled: &Arc<AtomicBool>,
    evt: UiEvent,
) {
    if let Ok(mut p) = pending.lock() {
        p.push(evt);
    }

    if scheduled
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return;
    }
    schedule_log_flush(ui.clone(), Arc::clone(pending), Arc::clone(scheduled));
}

// Détecte les dépôts tiers candidats dans /etc/apt/sources.list.d.
fn detect_third_party_candidates() -> Vec<String> {
    let dir = Path::new("/etc/apt/sources.list.d");
    let mut files = Vec::new();

    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(_) => return files,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let ext = match path.extension().and_then(|e| e.to_str()) {
            Some(ext) => ext,
            None => continue,
        };
        if ext == "list" || ext == "sources" {
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                if name == "debian.sources" {
                    continue;
                }
                files.push(name.to_string());
            }
        }
    }

    files.sort();
    files
}

// Point d'entrée GUI: initialise la fenêtre, les callbacks et le workflow multi-pages.
fn main() -> Result<(), slint::PlatformError> {
    let mode = parse_run_mode();
    let session_cfg = load_or_init_session_config(mode.debug);
    let app = create_app_window_with_backend_fallback()?;
    let window = app.window();
    // Centered startup position (desktop standard 1920x1080 baseline).
    // Slint 1.5 does not expose monitor geometry directly in this API surface.
    let size = window.size();
    let x = ((1920 - size.width as i32) / 2).max(0);
    let y = ((1080 - size.height as i32) / 2).max(0);
    window.set_position(slint::PhysicalPosition::new(x, y));

    let third_party = detect_third_party_candidates();
    let third_party_model = Rc::new(slint::VecModel::from(
        third_party
            .iter()
            .map(|name| ThirdPartyRepo {
                name: name.clone().into(),
                enabled: session_cfg.selected_third_party_repos.iter().any(|n| n == name),
            })
            .collect::<Vec<_>>(),
    ));
    let third_party_count = third_party.len();
    app.set_third_party_repos(slint::ModelRc::from(third_party_model.clone()));

    {
        let model = third_party_model.clone();
        app.on_set_third_party_enabled(move |index, enabled| {
            let idx = index as usize;
            if idx >= model.row_count() {
                return;
            }
            if let Some(mut row) = model.row_data(idx) {
                row.enabled = enabled;
                model.set_row_data(idx, row);
            }
        });
    }

    if mode.debug {
        append_log(&app, "[debug] Mode debug actif.");
    } else {
        append_log(&app, "[info] Mode normal actif.");
    }

    let check_release_running = Arc::new(AtomicBool::new(false));
    let validate_sources_running = Arc::new(AtomicBool::new(false));
    let prepare_packages_running = Arc::new(AtomicBool::new(false));
    let dry_run_upgrade_running = Arc::new(AtomicBool::new(false));

    {
        let weak = app.as_weak();
        app.on_go_next(move || {
            if let Some(app) = weak.upgrade() {
                let p = app.get_current_page();
                if p < 6 {
                    app.set_current_page(p + 1);
                }
            }
        });
    }

    {
        let weak = app.as_weak();
        app.on_go_prev(move || {
            if let Some(app) = weak.upgrade() {
                let p = app.get_current_page();
                if p > 1 {
                    app.set_current_page(p - 1);
                }
            }
        });
    }

    {
        let weak = app.as_weak();
        let check_release_running = Arc::clone(&check_release_running);
        app.on_check_new_release(move || {
            if check_release_running.swap(true, Ordering::SeqCst) {
                return;
            }
            let ui = weak.clone();
            let mode = mode;

            let _ = slint::invoke_from_event_loop({
                let ui = ui.clone();
                move || {
                    if let Some(app) = ui.upgrade() {
                        app.set_action_in_progress(true);
                        set_header_status(&app, "Verification version Debian en ligne...", StatusTone::Running);
                        append_log(&app, "[info] Verification de la nouvelle version majeure Debian...");
                    }
                }
            });

            let check_release_running_done = Arc::clone(&check_release_running);
            thread::spawn(move || {
                let ctx = AppContext {
                    dry_run: mode.debug,
                    debug: mode.debug,
                };
                let pending = Arc::new(Mutex::new(Vec::<UiEvent>::new()));
                let scheduled = Arc::new(AtomicBool::new(false));

                let mut publish = |evt: CoreEvent| {
                    let ui_evt = core_to_ui_event(evt);
                    publish_ui_event_batched(&ui, &pending, &scheduled, ui_evt);
                    Ok(())
                };

                let result = check_new_major_release(ctx, &mut publish);
                let _ = slint::invoke_from_event_loop(move || {
                    check_release_running_done.store(false, Ordering::SeqCst);
                    if let Some(app) = ui.upgrade() {
                        app.set_action_in_progress(false);
                        match result {
                            Ok(info) => {
                                if info.update_available {
                                    if third_party_count == 0 {
                                        append_log(&app, "[info] Aucun depot tiers detecte dans /etc/apt/sources.list.d.");
                                    } else {
                                        append_log(
                                            &app,
                                            &format!(
                                                "[info] {} depot(s) tiers detecte(s). La liste est scrollable pour tout afficher.",
                                                third_party_count
                                            ),
                                        );
                                    }
                                    append_log(
                                        &app,
                                        &format!(
                                            "[success] Mise a niveau disponible: {} -> {} ({})",
                                            info.current_major, info.stable_major, info.stable_codename
                                        ),
                                    );
                                    set_header_status(&app, "Nouvelle version detectee", StatusTone::Success);
                                    app.set_current_page(2);
                                } else if mode.debug {
                                    if third_party_count == 0 {
                                        append_log(&app, "[info] Aucun depot tiers detecte dans /etc/apt/sources.list.d.");
                                    } else {
                                        append_log(
                                            &app,
                                            &format!(
                                                "[info] {} depot(s) tiers detecte(s). La liste est scrollable pour tout afficher.",
                                                third_party_count
                                            ),
                                        );
                                    }
                                    append_log(
                                        &app,
                                        "[warn] Aucune nouvelle version detectee, mais mode debug actif: poursuite autorisee.",
                                    );
                                    set_header_status(&app, "Mode debug: poursuite test", StatusTone::Warn);
                                    app.set_current_page(2);
                                } else {
                                    append_log(
                                        &app,
                                        &format!(
                                            "[warn] Pas de nouvelle version: stable actuelle {} ({})",
                                            info.stable_major, info.stable_codename
                                        ),
                                    );
                                    set_header_status(&app, "Aucune nouvelle version", StatusTone::Warn);
                                    app.set_current_page(6);
                                }
                            }
                            Err(err) => {
                                append_log(&app, &format!("[error] Echec verification en ligne: {err}"));
                                set_header_status(&app, "Erreur verification version", StatusTone::Error);
                                app.set_current_page(6);
                            }
                        }
                    }
                });
            });
        });
    }

    {
        let weak = app.as_weak();
        let validate_sources_running = Arc::clone(&validate_sources_running);
        app.on_validate_sources(move || {
            if validate_sources_running.swap(true, Ordering::SeqCst) {
                return;
            }
            let ui = weak.clone();
            let mode = mode;
            let selected_for_reactivation = if let Some(app) = ui.upgrade() {
                let mut selected = Vec::<String>::new();
                let repos = app.get_third_party_repos();
                for idx in 0..repos.row_count() {
                    if let Some(repo) = repos.row_data(idx) {
                        if repo.enabled {
                            selected.push(repo.name.to_string());
                        }
                    }
                }
                selected
            } else {
                Vec::new()
            };

            let _ = slint::invoke_from_event_loop({
                let ui = ui.clone();
                move || {
                    if let Some(app) = ui.upgrade() {
                        app.set_action_in_progress(true);
                        set_header_status(&app, "Validation sources APT", StatusTone::Running);
                        append_log(&app, "[info] Validation des sources officielles...");
                    }
                }
            });

            let validate_sources_running_done = Arc::clone(&validate_sources_running);
            thread::spawn(move || {
                let pending = Arc::new(Mutex::new(Vec::<UiEvent>::new()));
                let scheduled = Arc::new(AtomicBool::new(false));

                let result = if mode.debug {
                    let ctx = AppContext {
                        dry_run: true,
                        debug: true,
                    };
                    let mut publish = |evt: CoreEvent| {
                        let ui_evt = core_to_ui_event(evt);
                        publish_ui_event_batched(&ui, &pending, &scheduled, ui_evt);
                        Ok(())
                    };
                    run_command(ctx, CoreCommand::CheckSources, &mut publish)
                        .and_then(|_| run_command(ctx, CoreCommand::DisableThirdParty, &mut publish))
                } else {
                    let keep_cmd = if selected_for_reactivation.is_empty() {
                        "set-third-party-reactivation".to_string()
                    } else {
                        format!(
                            "set-third-party-reactivation {}",
                            selected_for_reactivation.join(",")
                        )
                    };
                    let ui_stream = ui.clone();
                    run_backend_subcommands_via_privileged_backend_stream(
                        false,
                        &[
                            keep_cmd,
                            "check-sources".to_string(),
                            "disable-third-party".to_string(),
                        ],
                        move |evt| {
                            publish_ui_event_batched(
                                &ui_stream,
                                &pending,
                                &scheduled,
                                evt,
                            );
                        },
                    )
                };

                let _ = slint::invoke_from_event_loop(move || {
                    validate_sources_running_done.store(false, Ordering::SeqCst);
                    if let Some(app) = ui.upgrade() {
                        app.set_action_in_progress(false);
                        match result {
                            Ok(()) => {
                                let mut enabled = Vec::new();
                                let repos = app.get_third_party_repos();
                                for idx in 0..repos.row_count() {
                                    if let Some(repo) = repos.row_data(idx) {
                                        if repo.enabled {
                                            enabled.push(repo.name.to_string());
                                        }
                                    }
                                }

                                if enabled.is_empty() {
                                    append_log(&app, "[info] Aucun depot tiers selectionne pour reactivation post-upgrade.");
                                } else {
                                    append_log(&app, &format!("[info] Depots tiers selectionnes pour reactivation post-upgrade: {}", enabled.join(", ")));
                                }
                                let cfg = AppSessionConfig {
                                    debug_mode: mode.debug,
                                    selected_third_party_repos: enabled.clone(),
                                };
                                if let Err(err) = save_session_config(&cfg) {
                                    append_log(&app, &format!("[warn] Impossible de sauvegarder la session: {err}"));
                                } else {
                                    append_log(&app, "[info] Preferences UI sauvegardees (selection des depots tiers).");
                                }

                                app.set_current_page(3);
                                set_header_status(&app, "Sources validees", StatusTone::Success);
                            }
                            Err(err) => {
                                append_log(&app, &format!("[error] Echec validation sources: {err}"));
                                set_header_status(&app, "Erreur validation sources", StatusTone::Error);
                            }
                        }
                    }
                });
            });
        });
    }

    {
        let weak = app.as_weak();
        let prepare_packages_running = Arc::clone(&prepare_packages_running);
        app.on_run_download_step(move || {
            if prepare_packages_running.swap(true, Ordering::SeqCst) {
                return;
            }
            let ui = weak.clone();
            let mode = mode;

            let _ = slint::invoke_from_event_loop({
                let ui = ui.clone();
                move || {
                    if let Some(app) = ui.upgrade() {
                        app.set_action_in_progress(true);
                        set_header_status(&app, "Telechargement des paquets en cours", StatusTone::Running);
                        append_log(&app, "[info] Demarrage preparation paquets via upgrade-core...");
                    }
                }
            });

            let prepare_packages_running_done = Arc::clone(&prepare_packages_running);
            thread::spawn(move || {
                let ctx = AppContext {
                    dry_run: mode.debug,
                    debug: mode.debug,
                };
                let pending = Arc::new(Mutex::new(Vec::<UiEvent>::new()));
                let scheduled = Arc::new(AtomicBool::new(false));

                let mut publish = |evt: CoreEvent| {
                    let ui_evt = core_to_ui_event(evt);
                    publish_ui_event_batched(&ui, &pending, &scheduled, ui_evt);
                    Ok(())
                };

                let result = run_command(ctx, CoreCommand::PreparePackages, &mut publish);
                let _ = slint::invoke_from_event_loop(move || {
                    prepare_packages_running_done.store(false, Ordering::SeqCst);
                    if let Some(app) = ui.upgrade() {
                        app.set_action_in_progress(false);
                        match result {
                            Ok(()) => {
                                append_log(&app, "[success] Preparation des paquets terminee.");
                                set_header_status(&app, "Preparation paquets terminee", StatusTone::Success);
                                app.set_current_page(4);
                            }
                            Err(err) => {
                                let is_permission = format!("{err}").contains("Permission denied")
                                    || format!("{err}").contains("permission denied")
                                    || format!("{err}").contains("apt-get");
                                if is_permission && !mode.debug {
                                    append_log(&app, "[warn] Permissions insuffisantes detectees, tentative avec elevation privilegiee...");
                                    app.set_action_in_progress(true);
                                    set_header_status(&app, "Preparation des paquets en cours...", StatusTone::Running);
                                    let ui2 = app.as_weak();
                                    thread::spawn(move || {
                                        let pending = Arc::new(Mutex::new(Vec::<UiEvent>::new()));
                                        let scheduled = Arc::new(AtomicBool::new(false));
                                        let ui3 = ui2.clone();
                                        let privileged = run_backend_subcommands_via_privileged_backend_stream(
                                            false,
                                            &["prepare-packages".to_string()],
                                            move |evt| {
                                                publish_ui_event_batched(
                                                    &ui3,
                                                    &pending,
                                                    &scheduled,
                                                    evt,
                                                );
                                            },
                                        );
                                        let _ = slint::invoke_from_event_loop(move || {
                                            if let Some(app) = ui2.upgrade() {
                                                match privileged {
                                                    Ok(()) => {
                                                        set_header_status(&app, "Preparation paquets terminee", StatusTone::Success);
                                                        app.set_current_page(4);
                                                    }
                                                    Err(p_err) => {
                                                        append_log(&app, &format!("[error] Echec preparation privilegiee: {p_err}"));
                                                        set_header_status(&app, "Erreur preparation paquets", StatusTone::Error);
                                                    }
                                                }
                                            }
                                        });
                                    });
                                    return;
                                }
                                append_log(&app, &format!("[error] Echec preparation paquets: {err}"));
                                set_header_status(&app, "Erreur preparation paquets", StatusTone::Error);
                            }
                        }
                    }
                });
            });
        });
    }

    {
        let weak = app.as_weak();
        let dry_run_upgrade_running = Arc::clone(&dry_run_upgrade_running);
        app.on_run_dry_run_upgrade(move || {
            if dry_run_upgrade_running.swap(true, Ordering::SeqCst) {
                return;
            }
            let ui = weak.clone();
            let mode = mode;

            let _ = slint::invoke_from_event_loop({
                let ui = ui.clone();
                move || {
                    if let Some(app) = ui.upgrade() {
                        app.set_action_in_progress(true);
                        set_header_status(&app, "Test dry-run upgrade en cours", StatusTone::Running);
                        append_log(&app, "[info] Simulation du test de mise a niveau...");
                        append_log(&app, "[debug] Commande cible future: apt-get -s dist-upgrade");
                    }
                }
            });

            let dry_run_upgrade_running_done = Arc::clone(&dry_run_upgrade_running);
            thread::spawn(move || {
                let ctx = AppContext {
                    dry_run: mode.debug,
                    debug: mode.debug,
                };
                let mut publish = |evt: CoreEvent| {
                    let ui_evt = core_to_ui_event(evt);
                    let ui_clone = ui.clone();
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(app) = ui_clone.upgrade() {
                            apply_ui_event(&app, ui_evt);
                        }
                    });
                    Ok(())
                };
                let result = run_command(ctx, CoreCommand::DryRunUpgrade, &mut publish);

                let _ = slint::invoke_from_event_loop(move || {
                    dry_run_upgrade_running_done.store(false, Ordering::SeqCst);
                    if let Some(app) = ui.upgrade() {
                        app.set_action_in_progress(false);
                        match result {
                            Ok(()) => {
                                append_log(&app, "[success] Dry-run upgrade valide: aucune erreur bloquante detectee.");
                                set_header_status(&app, "Dry-run valide", StatusTone::Success);
                                app.set_current_page(5);
                            }
                            Err(err) => {
                                let is_permission = format!("{err}").contains("Permission denied")
                                    || format!("{err}").contains("permission denied")
                                    || format!("{err}").contains("apt-get");
                                if is_permission && !mode.debug {
                                    append_log(&app, "[warn] Permissions insuffisantes detectees, tentative avec elevation privilegiee...");
                                    app.set_action_in_progress(true);
                                    set_header_status(&app, "Elevation privilegiee...", StatusTone::Running);
                                    let ui2 = app.as_weak();
                                    thread::spawn(move || {
                                        let pending = Arc::new(Mutex::new(Vec::<UiEvent>::new()));
                                        let scheduled = Arc::new(AtomicBool::new(false));
                                        let ui3 = ui2.clone();
                                        let privileged = run_backend_subcommands_via_privileged_backend_stream(
                                            false,
                                            &["dry-run-upgrade".to_string()],
                                            move |evt| {
                                                publish_ui_event_batched(
                                                    &ui3,
                                                    &pending,
                                                    &scheduled,
                                                    evt,
                                                );
                                            },
                                        );
                                        let _ = slint::invoke_from_event_loop(move || {
                                            if let Some(app) = ui2.upgrade() {
                                                match privileged {
                                                    Ok(()) => {
                                                        set_header_status(&app, "Dry-run valide", StatusTone::Success);
                                                        app.set_current_page(5);
                                                    }
                                                    Err(p_err) => {
                                                        append_log(&app, &format!("[error] Echec dry-run privilegie: {p_err}"));
                                                        set_header_status(&app, "Erreur dry-run", StatusTone::Error);
                                                    }
                                                }
                                            }
                                        });
                                    });
                                    return;
                                }
                                append_log(&app, &format!("[error] Echec dry-run upgrade: {err}"));
                                set_header_status(&app, "Erreur dry-run", StatusTone::Error);
                            }
                        }
                    }
                });
            });
        });
    }

    {
        let weak = app.as_weak();
        app.on_request_reboot(move || {
            if let Some(app) = weak.upgrade() {
                if mode.debug {
                    set_header_status(&app, "Pret au redemarrage (debug)", StatusTone::Warn);
                    append_log(&app, "[warn] Redemarrage demande (debug): aucune action systeme reelle executee.");
                    append_log(&app, "[info] Execution cible: mode non interactif avec options par defaut (DEBIAN_FRONTEND=noninteractive).");
                    return;
                }

                app.set_action_in_progress(true);
                set_header_status(&app, "Armemement upgrade hors-ligne...", StatusTone::Running);
                append_log(&app, "[info] Configuration du mode upgrade hors-ligne (system-update.target + script non interactif)...");

                let ui = app.as_weak();
                thread::spawn(move || {
                    let pending = Arc::new(Mutex::new(Vec::<UiEvent>::new()));
                    let scheduled = Arc::new(AtomicBool::new(false));
                    let ui_stream = ui.clone();
                    let result = run_backend_subcommands_via_privileged_backend_stream(
                        false,
                        &["arm-and-reboot".to_string()],
                        move |evt| {
                            publish_ui_event_batched(
                                &ui_stream,
                                &pending,
                                &scheduled,
                                evt,
                            );
                        },
                    );
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(app) = ui.upgrade() {
                            match result {
                                Ok(()) => {
                                    append_log(&app, "[success] Upgrade hors-ligne arme. Redemarrage en cours...");
                                }
                                Err(err) => {
                                    app.set_action_in_progress(false);
                                    set_header_status(&app, "Erreur armer/reboot", StatusTone::Error);
                                    append_log(&app, &format!("[error] {err}"));
                                }
                            }
                        }
                    });
                });
            }
        });
    }

    {
        let weak = app.as_weak();
        app.on_close_app(move || {
            if let Some(app) = weak.upgrade() {
                let _ = app.window().hide();
            }
        });
    }

    app.run()
}
