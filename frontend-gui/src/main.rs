use std::thread;
use std::io::{BufRead, BufReader};
use std::{fs, path::Path};
use std::collections::VecDeque;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::sync::OnceLock;

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

static DEFERRED_LOG_LINES: OnceLock<Mutex<Vec<String>>> = OnceLock::new();
static LOG_RING: OnceLock<Mutex<VecDeque<String>>> = OnceLock::new();
const MAX_LOG_LINES: usize = 1200;
const LOG_FLUSH_INTERVAL_MS: u64 = 40;
const LOG_LINES_PER_FLUSH: usize = 8;

fn deferred_log_lines() -> &'static Mutex<Vec<String>> {
    DEFERRED_LOG_LINES.get_or_init(|| Mutex::new(Vec::new()))
}

fn log_ring() -> &'static Mutex<VecDeque<String>> {
    LOG_RING.get_or_init(|| Mutex::new(VecDeque::new()))
}

#[derive(Clone, Copy)]
struct RunMode {
    debug: bool,
}

fn parse_run_mode() -> RunMode {
    let debug = std::env::args().any(|arg| arg == "--debug");
    RunMode { debug }
}

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

fn run_with_pkexec(script: &str) -> anyhow::Result<()> {
    let status = Command::new("pkexec")
        .arg("/bin/sh")
        .arg("-c")
        .arg(script)
        .status()
        .map_err(|e| anyhow::anyhow!("pkexec indisponible ou echec lancement: {e}"))?;

    if status.success() {
        Ok(())
    } else {
        Err(anyhow::anyhow!("pkexec a echoue: {status}"))
    }
}

fn run_with_pkexec_stream<F>(program: &str, args: &[&str], mut on_stdout_line: F) -> anyhow::Result<()>
where
    F: FnMut(&str),
{
    let mut child = Command::new("pkexec")
        .arg(program)
        .args(args)
        .stdout(Stdio::piped())
        // Avoid pipe deadlocks on noisy stderr during long apt runs.
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| anyhow::anyhow!("pkexec indisponible ou echec lancement: {e}"))?;

    if let Some(stdout) = child.stdout.take() {
        let reader = BufReader::new(stdout);
        for line in reader.lines().map_while(Result::ok) {
            let l = line.trim();
            if !l.is_empty() {
                on_stdout_line(l);
            }
        }
    }

    let status = child
        .wait()
        .map_err(|e| anyhow::anyhow!("pkexec echec attente: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(anyhow::anyhow!("pkexec a echoue: {status}"))
    }
}

fn run_with_zenity_fallback(script: &str) -> anyhow::Result<()> {
    let output = Command::new("zenity")
        .arg("--password")
        .arg("--title=Authentification requise")
        .output()
        .map_err(|e| anyhow::anyhow!("zenity indisponible: {e}"))?;

    if !output.status.success() {
        return Err(anyhow::anyhow!("zenity annule ou en echec"));
    }

    let mut password = String::from_utf8(output.stdout)
        .map_err(|e| anyhow::anyhow!("mot de passe zenity invalide UTF-8: {e}"))?;
    while password.ends_with('\n') || password.ends_with('\r') {
        password.pop();
    }

    let mut child = Command::new("sudo")
        .arg("-S")
        .arg("/bin/sh")
        .arg("-c")
        .arg(script)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| anyhow::anyhow!("sudo fallback impossible a lancer: {e}"))?;

    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(format!("{password}\n").as_bytes());
    }
    let status = child
        .wait()
        .map_err(|e| anyhow::anyhow!("sudo fallback echec attente: {e}"))?;

    // Effacement explicite du champ mot de passe en memoire.
    password.replace_range(.., &"\0".repeat(password.len()));
    password.clear();

    if status.success() {
        Ok(())
    } else {
        Err(anyhow::anyhow!("sudo fallback a echoue: {status}"))
    }
}

fn run_with_zenity_fallback_stream<F>(program: &str, args: &[&str], mut on_stdout_line: F) -> anyhow::Result<()>
where
    F: FnMut(&str),
{
    let output = Command::new("zenity")
        .arg("--password")
        .arg("--title=Authentification requise")
        .output()
        .map_err(|e| anyhow::anyhow!("zenity indisponible: {e}"))?;

    if !output.status.success() {
        return Err(anyhow::anyhow!("zenity annule ou en echec"));
    }

    let mut password = String::from_utf8(output.stdout)
        .map_err(|e| anyhow::anyhow!("mot de passe zenity invalide UTF-8: {e}"))?;
    while password.ends_with('\n') || password.ends_with('\r') {
        password.pop();
    }

    let mut child = Command::new("sudo")
        .arg("-S")
        .arg(program)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        // Avoid pipe deadlocks on noisy stderr during long apt runs.
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| anyhow::anyhow!("sudo fallback impossible a lancer: {e}"))?;

    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(format!("{password}\n").as_bytes());
    }

    if let Some(stdout) = child.stdout.take() {
        let reader = BufReader::new(stdout);
        for line in reader.lines().map_while(Result::ok) {
            let l = line.trim();
            if !l.is_empty() {
                on_stdout_line(l);
            }
        }
    }

    let status = child
        .wait()
        .map_err(|e| anyhow::anyhow!("sudo fallback echec attente: {e}"))?;

    password.replace_range(.., &"\0".repeat(password.len()));
    password.clear();

    if status.success() {
        Ok(())
    } else {
        Err(anyhow::anyhow!("sudo fallback a echoue: {status}"))
    }
}

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

fn run_backend_subcommands_via_privileged_backend_stream<F>(
    debug: bool,
    subcommands: &[&str],
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

    let base_args = if debug { vec!["--dry-run", "--debug"] } else { vec![] };
    for sub in subcommands {
        let mut args = base_args.clone();
        args.push(sub);
        let args_refs: Vec<&str> = args.to_vec();

        let mut feed = |line: &str| {
            for evt in parse_backend_json_events(line.as_bytes()) {
                on_event(evt);
            }
        };

        if let Err(pk_err) = run_with_pkexec_stream(&bin_str, &args_refs, &mut feed) {
            run_with_zenity_fallback_stream(&bin_str, &args_refs, &mut feed)
                .map_err(|zen_err| anyhow::anyhow!("echec elevation privilegies (pkexec puis zenity): {pk_err}; {zen_err}"))?;
        }
    }
    Ok(())
}

fn setup_offline_upgrade_and_reboot() -> anyhow::Result<()> {
    let setup_script = r#"set -euo pipefail

if [ ! -x /usr/local/lib/debian-upgrade/offline-upgrade.sh ]; then
  echo "Script manquant: /usr/local/lib/debian-upgrade/offline-upgrade.sh"
  exit 1
fi

if [ ! -f /usr/lib/systemd/system/debian-upgrade-offline.service ]; then
  echo "Service manquant: /usr/lib/systemd/system/debian-upgrade-offline.service"
  exit 1
fi

install -d -m 0755 /var/lib/system-update
install -d -m 0755 /etc/systemd/system/system-update.target.wants
ln -snf /usr/lib/systemd/system/debian-upgrade-offline.service /etc/systemd/system/system-update.target.wants/debian-upgrade-offline.service
ln -snf /var/lib/system-update /system-update
systemctl daemon-reload
"#;
    if let Err(pk_err) = run_with_pkexec(setup_script) {
        run_with_zenity_fallback(&setup_script)
            .map_err(|zen_err| anyhow::anyhow!("echec elevation privilegies (pkexec puis zenity): {pk_err}; {zen_err}"))?;
    }

    let reboot_script = "systemctl reboot";
    if let Err(pk_err) = run_with_pkexec(reboot_script) {
        run_with_zenity_fallback(reboot_script)
            .map_err(|zen_err| anyhow::anyhow!("echec reboot privilegie (pkexec puis zenity): {pk_err}; {zen_err}"))?;
    }
    Ok(())
}

fn append_log(app: &AppWindow, line: &str) {
    append_logs_batch(app, &[line.to_string()]);
}

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

fn level_to_str(level: LogLevel) -> &'static str {
    match level {
        LogLevel::Debug => "debug",
        LogLevel::Info => "info",
        LogLevel::Warn => "warn",
        LogLevel::Error => "error",
        LogLevel::Success => "success",
    }
}

fn state_to_str(state: StepState) -> &'static str {
    match state {
        StepState::Pending => "pending",
        StepState::Running => "running",
        StepState::Done => "done",
        StepState::Failed => "failed",
    }
}

fn core_to_ui_event(event: CoreEvent) -> UiEvent {
    UiEvent {
        level: level_to_str(event.level).to_string(),
        step: event.step.to_string(),
        state: state_to_str(event.state).to_string(),
        message: event.message,
    }
}

fn apply_ui_event(app: &AppWindow, evt: UiEvent) {
    append_log(app, &format!("[{}] {} ({}, {})", evt.level, evt.message, evt.step, evt.state));
}

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

fn main() -> Result<(), slint::PlatformError> {
    let mode = parse_run_mode();
    let app = create_app_window_with_backend_fallback()?;
    let window = app.window();
    // Centered startup position (desktop standard 1920x1080 baseline).
    // Slint 1.5 does not expose monitor geometry directly in this API surface.
    let size = window.size();
    let x = ((1920 - size.width as i32) / 2).max(0);
    let y = ((1080 - size.height as i32) / 2).max(0);
    window.set_position(slint::PhysicalPosition::new(x, y));

    let third_party = detect_third_party_candidates();
    app.set_third_party_1_visible(false);
    app.set_third_party_2_visible(false);
    app.set_third_party_3_visible(false);
    app.set_third_party_1_enabled(false);
    app.set_third_party_2_enabled(false);
    app.set_third_party_3_enabled(false);

    if let Some(name) = third_party.first() {
        app.set_third_party_1_name(name.as_str().into());
        app.set_third_party_1_visible(true);
    }
    if let Some(name) = third_party.get(1) {
        app.set_third_party_2_name(name.as_str().into());
        app.set_third_party_2_visible(true);
    }
    if let Some(name) = third_party.get(2) {
        app.set_third_party_3_name(name.as_str().into());
        app.set_third_party_3_visible(true);
    }

    if third_party.len() > 3 {
        append_log(
            &app,
            &format!(
                "[info] {} depots detectes (.list/.sources), affichage limite aux 3 premiers.",
                third_party.len()
            ),
        );
    } else if third_party.is_empty() {
        append_log(&app, "[info] Aucun depot tiers detecte dans /etc/apt/sources.list.d.");
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
                        app.set_header_status("Verification version Debian en ligne...".into());
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
                        match result {
                            Ok(info) => {
                                if info.update_available {
                                    append_log(
                                        &app,
                                        &format!(
                                            "[success] Mise a niveau disponible: {} -> {} ({})",
                                            info.current_major, info.stable_major, info.stable_codename
                                        ),
                                    );
                                    app.set_header_status("Nouvelle version detectee".into());
                                    app.set_current_page(2);
                                } else if mode.debug {
                                    append_log(
                                        &app,
                                        "[warn] Aucune nouvelle version detectee, mais mode debug actif: poursuite autorisee.",
                                    );
                                    app.set_header_status("Mode debug: poursuite test".into());
                                    app.set_current_page(2);
                                } else {
                                    append_log(
                                        &app,
                                        &format!(
                                            "[warn] Pas de nouvelle version: stable actuelle {} ({})",
                                            info.stable_major, info.stable_codename
                                        ),
                                    );
                                    app.set_header_status("Aucune nouvelle version".into());
                                    app.set_current_page(6);
                                }
                            }
                            Err(err) => {
                                append_log(&app, &format!("[error] Echec verification en ligne: {err}"));
                                app.set_header_status("Erreur verification version".into());
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

            let _ = slint::invoke_from_event_loop({
                let ui = ui.clone();
                move || {
                    if let Some(app) = ui.upgrade() {
                        app.set_header_status("Validation sources APT".into());
                        append_log(&app, "[info] Validation des sources officielles...");
                    }
                }
            });

            let validate_sources_running_done = Arc::clone(&validate_sources_running);
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

                let result = run_command(ctx, CoreCommand::CheckSources, &mut publish)
                    .and_then(|_| run_command(ctx, CoreCommand::DisableThirdParty, &mut publish));

                let _ = slint::invoke_from_event_loop(move || {
                    validate_sources_running_done.store(false, Ordering::SeqCst);
                    if let Some(app) = ui.upgrade() {
                        match result {
                            Ok(()) => {
                                let mut enabled = Vec::new();
                                if app.get_third_party_1_visible() && app.get_third_party_1_enabled() {
                                    enabled.push(app.get_third_party_1_name().to_string());
                                }
                                if app.get_third_party_2_visible() && app.get_third_party_2_enabled() {
                                    enabled.push(app.get_third_party_2_name().to_string());
                                }
                                if app.get_third_party_3_visible() && app.get_third_party_3_enabled() {
                                    enabled.push(app.get_third_party_3_name().to_string());
                                }

                                if enabled.is_empty() {
                                    append_log(&app, "[info] Aucun depot tiers re-active. Tous les depots tiers restent desactives.");
                                } else {
                                    append_log(&app, &format!("[warn] Depots tiers re-actives manuellement: {}", enabled.join(", ")));
                                }

                                app.set_current_page(3);
                                app.set_header_status("Sources validees".into());
                            }
                            Err(err) => {
                                let is_permission = format!("{err}").contains("Permission denied")
                                    || format!("{err}").contains("permission denied")
                                    || format!("{err}").contains("ecriture /etc/apt");
                                if is_permission && !mode.debug {
                                    append_log(&app, "[warn] Permissions insuffisantes detectees, tentative avec elevation privilegiee...");
                                    app.set_header_status("Elevation privilegiee...".into());
                                    let ui2 = app.as_weak();
                                    thread::spawn(move || {
                                        let pending = Arc::new(Mutex::new(Vec::<UiEvent>::new()));
                                        let scheduled = Arc::new(AtomicBool::new(false));
                                        let ui3 = ui2.clone();
                                        let privileged = run_backend_subcommands_via_privileged_backend_stream(
                                            false,
                                            &["check-sources", "disable-third-party"],
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
                                                        app.set_current_page(3);
                                                        app.set_header_status("Sources validees".into());
                                                    }
                                                    Err(p_err) => {
                                                        append_log(&app, &format!("[error] Echec validation privilegiee: {p_err}"));
                                                        app.set_header_status("Erreur validation sources".into());
                                                    }
                                                }
                                            }
                                        });
                                    });
                                    return;
                                }
                                append_log(&app, &format!("[error] Echec validation sources: {err}"));
                                app.set_header_status("Erreur validation sources".into());
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
                        app.set_header_status("Telechargement des paquets en cours".into());
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
                        match result {
                            Ok(()) => {
                                append_log(&app, "[success] Preparation des paquets terminee.");
                                app.set_header_status("Preparation paquets terminee".into());
                                app.set_current_page(4);
                            }
                            Err(err) => {
                                let is_permission = format!("{err}").contains("Permission denied")
                                    || format!("{err}").contains("permission denied")
                                    || format!("{err}").contains("apt-get");
                                if is_permission && !mode.debug {
                                    append_log(&app, "[warn] Permissions insuffisantes detectees, tentative avec elevation privilegiee...");
                                    app.set_header_status("Elevation privilegiee...".into());
                                    let ui2 = app.as_weak();
                                    thread::spawn(move || {
                                        let pending = Arc::new(Mutex::new(Vec::<UiEvent>::new()));
                                        let scheduled = Arc::new(AtomicBool::new(false));
                                        let ui3 = ui2.clone();
                                        let privileged = run_backend_subcommands_via_privileged_backend_stream(
                                            false,
                                            &["prepare-packages"],
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
                                                        app.set_header_status("Preparation paquets terminee".into());
                                                        app.set_current_page(4);
                                                    }
                                                    Err(p_err) => {
                                                        append_log(&app, &format!("[error] Echec preparation privilegiee: {p_err}"));
                                                        app.set_header_status("Erreur preparation paquets".into());
                                                    }
                                                }
                                            }
                                        });
                                    });
                                    return;
                                }
                                append_log(&app, &format!("[error] Echec preparation paquets: {err}"));
                                app.set_header_status("Erreur preparation paquets".into());
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
                        app.set_header_status("Test dry-run upgrade en cours".into());
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
                        match result {
                            Ok(()) => {
                                append_log(&app, "[success] Dry-run upgrade valide: aucune erreur bloquante detectee.");
                                app.set_header_status("Dry-run valide".into());
                                app.set_current_page(5);
                            }
                            Err(err) => {
                                let is_permission = format!("{err}").contains("Permission denied")
                                    || format!("{err}").contains("permission denied")
                                    || format!("{err}").contains("apt-get");
                                if is_permission && !mode.debug {
                                    append_log(&app, "[warn] Permissions insuffisantes detectees, tentative avec elevation privilegiee...");
                                    app.set_header_status("Elevation privilegiee...".into());
                                    let ui2 = app.as_weak();
                                    thread::spawn(move || {
                                        let pending = Arc::new(Mutex::new(Vec::<UiEvent>::new()));
                                        let scheduled = Arc::new(AtomicBool::new(false));
                                        let ui3 = ui2.clone();
                                        let privileged = run_backend_subcommands_via_privileged_backend_stream(
                                            false,
                                            &["dry-run-upgrade"],
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
                                                        app.set_header_status("Dry-run valide".into());
                                                        app.set_current_page(5);
                                                    }
                                                    Err(p_err) => {
                                                        append_log(&app, &format!("[error] Echec dry-run privilegie: {p_err}"));
                                                        app.set_header_status("Erreur dry-run".into());
                                                    }
                                                }
                                            }
                                        });
                                    });
                                    return;
                                }
                                append_log(&app, &format!("[error] Echec dry-run upgrade: {err}"));
                                app.set_header_status("Erreur dry-run".into());
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
                    app.set_header_status("Pret au redemarrage (debug)".into());
                    append_log(&app, "[warn] Redemarrage demande (debug): aucune action systeme reelle executee.");
                    append_log(&app, "[info] Execution cible: mode non interactif avec options par defaut (DEBIAN_FRONTEND=noninteractive).");
                    return;
                }

                app.set_header_status("Armemement upgrade hors-ligne...".into());
                append_log(&app, "[info] Configuration du mode upgrade hors-ligne (system-update.target + script non interactif)...");

                let ui = app.as_weak();
                thread::spawn(move || {
                    let result = setup_offline_upgrade_and_reboot();
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(app) = ui.upgrade() {
                            match result {
                                Ok(()) => {
                                    append_log(&app, "[success] Upgrade hors-ligne arme. Redemarrage en cours...");
                                }
                                Err(err) => {
                                    app.set_header_status("Erreur armer/reboot".into());
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
