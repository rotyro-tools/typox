mod ai;
mod clipboard;
mod commands;
mod settings;
mod state;

use tauri::menu::{MenuBuilder, MenuItemBuilder, PredefinedMenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{Emitter, Manager, WindowEvent};
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};

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
        .plugin(tauri_plugin_opener::init())
        .manage(AppState::new())
        .setup(|app| {
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);
            // ── Tray menu ─────────────────────────────────────────────────────
            let settings_item = MenuItemBuilder::with_id("settings", "Settings").build(app)?;
            let sep = PredefinedMenuItem::separator(app)?;
            let quit_item = PredefinedMenuItem::quit(app, Some("Quit Typox"))?;

            let menu = MenuBuilder::new(app)
                .items(&[&settings_item, &sep, &quit_item])
                .build()?;

            let tray_icon = app
                .default_window_icon()
                .ok_or("no app icon configured")?
                .clone();

            TrayIconBuilder::new()
                .icon(tray_icon)
                .icon_as_template(true)
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

            // ── Global shortcut (user-configurable, loaded from settings) ────────
            // Falls back to Ctrl+Shift+Space if the saved value can't be parsed.
            let saved_shortcut_str = settings::load(app.handle()).shortcut;
            let shortcut = commands::parse_shortcut(&saved_shortcut_str).unwrap_or_else(|| {
                Shortcut::new(Some(Modifiers::CONTROL | Modifiers::SHIFT), Code::Space)
            });
            app.handle()
                .global_shortcut()
                .on_shortcut(shortcut, |app, _shortcut, event| {
                    if event.state() == ShortcutState::Pressed {
                        let settings_focused = app
                            .get_webview_window("settings")
                            .and_then(|w| w.is_focused().ok())
                            .unwrap_or(false);
                        if settings_focused {
                            let current = settings::load(app).shortcut;
                            let _ = app.emit_to("settings", "shortcut-triggered", current);
                            return;
                        }
                        // Skip if a correction is already in progress
                        let busy = app
                            .get_webview_window("widget")
                            .and_then(|w| w.is_visible().ok())
                            .unwrap_or(true);
                        if busy {
                            return;
                        }
                        let handle = app.clone();
                        tauri::async_runtime::spawn(async move {
                            clipboard::capture_and_correct(handle).await;
                        });
                    }
                })?;

            // ── Non-blocking startup update check ─────────────────────────
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                commands::run_update_check(handle).await;
            });

            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                if window.label() == "widget" || window.label() == "settings" {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_settings,
            commands::save_settings,
            commands::get_api_key,
            commands::save_api_key,
            commands::get_ollama_models,
            commands::get_claude_models,
            commands::get_openai_models,
            commands::get_default_prompt,
            commands::get_app_version,
            commands::apply_correction,
            commands::dismiss_correction,
            commands::retry_correction,
            commands::check_update,
            commands::update_shortcut,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
