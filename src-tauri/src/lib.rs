mod ai;
mod capture;
mod commands;
mod platform;
mod secrets;
mod settings;
mod shortcut;
mod state;
mod widget;

use tauri::menu::{MenuBuilder, MenuItemBuilder, PredefinedMenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{Emitter, Manager, WindowEvent};
use tauri_plugin_notification::NotificationExt;

use state::AppState;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_store::Builder::default().build())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .plugin(tauri_plugin_notification::init())
        .manage(AppState::new())
        .setup(|app| {
            // A menu-bar app: no Dock icon.
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            // ── Tray menu ─────────────────────────────────────────────────────
            let settings_item = MenuItemBuilder::with_id("settings", "Settings").build(app)?;
            let sep = PredefinedMenuItem::separator(app)?;
            let quit_item = PredefinedMenuItem::quit(app, Some("Quit Typox"))?;
            let menu = MenuBuilder::new(app)
                .items(&[&settings_item, &sep, &quit_item])
                .build()?;

            // The icon is a silhouette on transparency, so macOS can tint it as a template.
            let tray_icon = app
                .default_window_icon()
                .ok_or("no app icon configured")?
                .clone();
            TrayIconBuilder::new()
                .icon(tray_icon)
                .icon_as_template(true)
                .tooltip("Typox")
                .menu(&menu)
                .on_menu_event(|app, event| {
                    if event.id().as_ref() == "settings" {
                        if let Some(win) = app.get_webview_window("settings") {
                            let _ = app.emit_to("settings", "settings-opened", ());
                            let _ = win.show();
                            let _ = win.set_focus();
                        }
                    }
                })
                .build(app)?;

            // ── API keys: move any plain-text key from older versions to the keychain ──
            let handle = app.handle().clone();
            tauri::async_runtime::spawn_blocking(move || secrets::migrate_legacy(&handle));

            // ── Global shortcut ───────────────────────────────────────────────
            if let Err(e) = shortcut::register_saved(app.handle()) {
                eprintln!("[typox] no global shortcut: {e}");
                let _ = app
                    .notification()
                    .builder()
                    .title("Typox has no shortcut")
                    .body("Its shortcut is used by another app. Pick a new one in Settings.")
                    .show();
            }

            // ── Non-blocking startup update check ─────────────────────────────
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                commands::run_update_check(handle).await;
            });

            Ok(())
        })
        .on_window_event(|window, event| {
            let WindowEvent::CloseRequested { api, .. } = event else {
                return;
            };
            // Both windows live for the whole run; closing only hides them.
            api.prevent_close();
            if window.label() == widget::LABEL {
                // Same as Dismiss: give back the clipboard and focus.
                let app = window.app_handle().clone();
                tauri::async_runtime::spawn(async move { capture::dismiss(&app).await });
            } else {
                let _ = window.hide();
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_settings,
            commands::save_settings,
            commands::get_api_key_status,
            commands::save_api_key,
            commands::delete_api_key,
            commands::get_ollama_models,
            commands::get_claude_models,
            commands::get_openai_models,
            commands::get_default_prompt,
            commands::get_app_version,
            commands::apply_correction,
            commands::dismiss_correction,
            commands::retry_correction,
            commands::fit_widget,
            commands::check_update,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
