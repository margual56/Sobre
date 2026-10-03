pub mod accounts;
pub mod actions;
pub mod commands;
pub mod db;
pub mod icons;
pub mod mail;
pub mod net;
pub mod render;
pub mod state;
pub mod trust;

use std::sync::Arc;

use tauri::{
    menu::{Menu, MenuItem},
    tray::TrayIconBuilder,
    AppHandle, Emitter, Manager, RunEvent, WebviewUrl, WebviewWindowBuilder, WindowEvent,
};
use tauri_plugin_notification::NotificationExt as _;

use db::key::{KeyConfig, KeyMode};
use state::{AppState, Event};

/// The tray needs an AppIndicator library that many desktops do not ship.
pub fn tray_available() -> bool {
    if !cfg!(target_os = "linux") {
        return true;
    }
    [
        "/usr/lib",
        "/usr/lib64",
        "/usr/lib/x86_64-linux-gnu",
        "/usr/lib/aarch64-linux-gnu",
        "/lib/x86_64-linux-gnu",
    ]
    .iter()
    .flat_map(|dir| {
        ["libayatana-appindicator3.so.1", "libappindicator3.so.1"].map(|lib| format!("{dir}/{lib}"))
    })
    .any(|path| std::path::Path::new(&path).exists())
}

fn show_main(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        window.show().ok();
        window.unminimize().ok();
        window.set_focus().ok();
    }
}

pub(crate) fn allow_navigation(app: &AppHandle, token: &str, url: &url::Url) -> bool {
    match url.scheme() {
        "tauri" | "about" => true,
        "mailbody" => url.path().starts_with(&format!("/{token}/")),
        "http" | "https"
            if matches!(
                url.host_str(),
                Some("localhost" | "127.0.0.1" | "tauri.localhost")
            ) =>
        {
            true
        }
        "http" | "https" | "mailto" => {
            app.emit("link-clicked", url.as_str()).ok();
            false
        }
        _ => false,
    }
}

fn build_tray(app: &AppHandle) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open", "Open", true, None::<&str>)?;
    let check = MenuItem::with_id(app, "check", "Check now", true, None::<&str>)?;
    let compose = MenuItem::with_id(app, "compose", "New message", true, None::<&str>)?;
    let lock = MenuItem::with_id(app, "lock", "Lock now", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &check, &compose, &lock, &quit])?;
    TrayIconBuilder::with_id("main")
        .icon(
            app.default_window_icon()
                .cloned()
                .ok_or(tauri::Error::InvalidIcon(std::io::Error::other("no icon")))?,
        )
        .tooltip("Sobre")
        .menu(&menu)
        .on_menu_event(|app, event| {
            let state = app.state::<Arc<AppState>>();
            match event.id().as_ref() {
                "open" => show_main(app),
                "check" => {
                    for (_, tx) in state.syncers.lock().unwrap().iter() {
                        tx.send(state::SyncCmd::Poke).ok();
                    }
                }
                "compose" => {
                    show_main(app);
                    app.emit("compose", ()).ok();
                }
                "lock" => {
                    if state.mode() == Some(KeyMode::Passphrase) {
                        state.lock();
                    }
                }
                "quit" => app.exit(0),
                _ => {}
            }
        })
        .build(app)?;
    Ok(())
}

fn on_event(app: &AppHandle, state: &AppState, event: Event) {
    match &event {
        Event::NewMail {
            from,
            subject,
            count,
            ..
        } => {
            let enabled = state
                .with_db(|c| db::get_setting(c, "notifications"))
                .ok()
                .flatten()
                .as_deref()
                != Some("0");
            if enabled {
                let title = if *count > 1 {
                    format!("{count} new messages")
                } else {
                    from.clone()
                };
                app.notification()
                    .builder()
                    .title(title)
                    .body(subject)
                    .show()
                    .ok();
            }
        }
        Event::Locked => {
            // Compose windows hold decrypted text; they go when the store locks.
            for (label, window) in app.webview_windows() {
                if label.starts_with("compose-") {
                    window.destroy().ok();
                }
            }
        }
        _ => {}
    }
    match &event {
        Event::MailChanged { .. } | Event::Locked => {
            if let Some(tray) = app.tray_by_id("main") {
                let tip = match state.with_db(db::store::unread_count) {
                    Ok(0) => "Sobre".to_string(),
                    Ok(n) => format!("Sobre: {n} unread"),
                    Err(_) => "Sobre (locked)".to_string(),
                };
                tray.set_tooltip(Some(tip)).ok();
            }
        }
        _ => {}
    }
    app.emit("backend", &event).ok();
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Several dependencies bring their own TLS backends; pick one for the process.
    rustls::crypto::aws_lc_rs::default_provider()
        .install_default()
        .ok();

    let app = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            show_main(app)
        }))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec!["--minimized"]),
        ))
        .register_asynchronous_uri_scheme_protocol("mailbody", |ctx, request, responder| {
            let state = ctx.app_handle().state::<Arc<AppState>>().inner().clone();
            let uri = request.uri().to_string();
            tauri::async_runtime::spawn(async move {
                responder.respond(render::protocol::handle(state, uri).await);
            });
        })
        .invoke_handler(tauri::generate_handler![
            commands::app_status,
            commands::create_store,
            commands::unlock,
            commands::lock,
            commands::set_key_mode,
            commands::discover_account,
            commands::add_account,
            commands::list_accounts,
            commands::remove_account,
            commands::get_oauth_client,
            commands::set_oauth_client,
            commands::list_folders,
            commands::list_messages,
            commands::unread_count,
            commands::get_message,
            commands::sender_icon,
            commands::quote_text,
            commands::unsubscribe,
            commands::set_flag,
            commands::move_messages,
            commands::block_sender,
            commands::unblock_sender,
            commands::list_blocked,
            commands::trust_images,
            commands::sync_now,
            commands::load_older,
            commands::open_link,
            commands::save_attachment,
            commands::open_attachment,
            commands::pick_files,
            commands::send_message,
            commands::open_compose_window,
            commands::take_compose_draft,
            commands::close_window,
            commands::get_stats,
            commands::clear_storage,
            commands::get_settings,
            commands::set_settings,
        ])
        .setup(|app| {
            let handle = app.handle().clone();
            let state = Arc::new(AppState::new(app.path().app_data_dir()?)?);
            let sink_state = Arc::downgrade(&state);
            let sink_app = handle.clone();
            state.set_sink(Arc::new(move |event| {
                if let Some(state) = sink_state.upgrade() {
                    on_event(&sink_app, &state, event);
                }
            }));
            app.manage(state.clone());

            // Wallet mode opens without asking; passphrase mode waits for the user.
            if let Ok(Some(KeyConfig {
                mode: KeyMode::Wallet,
                ..
            })) = KeyConfig::load(&state.data_dir)
            {
                match state.unlock(None) {
                    Ok(()) => {
                        let state = state.clone();
                        tauri::async_runtime::spawn(async move { mail::sync::start_all(&state) });
                    }
                    Err(e) => log::error!("could not open the mail store: {e:#}"),
                }
            }

            let has_tray = tray_available()
                && build_tray(&handle)
                    .map_err(|e| log::warn!("no tray icon: {e}"))
                    .is_ok();
            let minimized = has_tray && std::env::args().any(|a| a == "--minimized");
            let nav_app = handle.clone();
            let token = state.token.clone();
            let window = WebviewWindowBuilder::new(app, "main", WebviewUrl::default())
                .title("Sobre")
                .inner_size(1180.0, 760.0)
                .min_inner_size(720.0, 420.0)
                .visible(!minimized)
                // No cookies, cache or storage written to disk by the webview.
                .incognito(true)
                .on_navigation(move |url| allow_navigation(&nav_app, &token, url))
                .build()?;
            let hide_target = window.clone();
            window.on_window_event(move |event| {
                if let WindowEvent::CloseRequested { api, .. } = event {
                    // With a tray the app keeps running in the background.
                    if has_tray {
                        api.prevent_close();
                        hide_target.hide().ok();
                    }
                }
            });
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building the application");

    app.run(|app, event| {
        if let RunEvent::Exit = event {
            app.state::<Arc<AppState>>().remove_temp_files();
        }
    });
}
