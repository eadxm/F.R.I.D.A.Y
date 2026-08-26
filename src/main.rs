mod tools;
mod audio;
mod ai;

#![windows_subsystem = "windows"]

slint::include_modules!();
use rdev::{listen, Event, EventType, Key};
use reqwest::Client;
use std::sync::{Arc, Mutex};
use std::thread;
use slint::{ComponentHandle, SharedString, Weak};
use tray_icon::{TrayIconBuilder, menu::{Menu, MenuItem, MenuEvent}, Icon};

fn detect_windows_dark_theme() -> bool {
    #[cfg(target_os = "windows")]
    {
        use std::process::Command;
        let output = Command::new("powershell")
            .args(["-Command", "Get-ItemPropertyValue -Path 'HKCU:\\Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize' -Name AppsUseLightTheme -ErrorAction SilentlyContinue"])
            .output();

        if let Ok(out) = output {
            let res = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if res == "1" { return false; }
        }
    }
    true
}

async fn validate_gemini_key(client: &Client, key: &str) -> bool {
    let url = format!("https://generativelanguage.googleapis.com/v1beta/models?key={}", key);
    match client.get(&url).send().await {
        Ok(res) => res.status().is_success(),
        Err(_) => false,
    }
}

// Spawns a lightweight System Tray icon to allow safe quitting
fn spawn_system_tray() {
    // Generate a simple 2x2 solid blue square for the tray icon
    let rgba = vec![
        0, 210, 255, 255,   0, 210, 255, 255,
        0, 210, 255, 255,   0, 210, 255, 255,
    ];
    let icon = Icon::from_rgba(rgba, 2, 2).unwrap();

    let tray_menu = Menu::new();
    let quit_item = MenuItem::new("Quit F.R.I.D.A.Y.", true, None);
    let quit_id = quit_item.id().clone();
    tray_menu.append(&quit_item).unwrap();

    let tray_icon = TrayIconBuilder::new()
        .with_menu(Box::new(tray_menu))
        .with_tooltip("F.R.I.D.A.Y. Core")
        .with_icon(icon)
        .build()
        .unwrap();

    // Background thread to listen for the tray "Quit" click
    thread::spawn(move || {
        loop {
            if let Ok(event) = MenuEvent::receiver().try_recv() {
                if event.id == quit_id {
                    std::process::exit(0);
                }
            }
            thread::sleep(std::time::Duration::from_millis(100));
        }
    });

    Box::leak(Box::new(tray_icon)); 
}

#[tokio::main]
async fn main() -> Result<(), slint::PlatformError> {
    let is_dark_system = detect_windows_dark_theme();
    let http_client = Client::new();

    let setup_ui = FridaySetup::new()?;
    let setup_weak = setup_ui.as_weak();
    setup_ui.set_is_dark(is_dark_system);

    setup_ui.on_dismiss_window({
        let setup_weak = setup_weak.clone();
        move || { if let Some(ui) = setup_weak.upgrade() { let _ = ui.hide(); } }
    });

    setup_ui.on_exit_app(|| { std::process::exit(0); });

    setup_ui.on_initialize({
        let setup_weak = setup_weak.clone();
        let http_client = http_client.clone();

        move |gemini_key, eleven_key| {
            let key = gemini_key.trim().to_string();
            let setup_weak = setup_weak.clone();
            let http_client = http_client.clone();

            if key.is_empty() {
                if let Some(ui) = setup_weak.upgrade() { ui.set_error_msg(SharedString::from("Gemini API key is required.")); }
                return;
            }

            if let Some(ui) = setup_weak.upgrade() {
                ui.set_is_validating(true);
                ui.set_error_msg(SharedString::from(""));
            }

            tokio::spawn(async move {
                let is_valid = validate_gemini_key(&http_client, &key).await;

                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(ui) = setup_weak.upgrade() {
                        ui.set_is_validating(false);

                        if !is_valid {
                            ui.set_error_msg(SharedString::from("Invalid API key or network error."));
                            return;
                        }

                        // Success -> Initialize System Tray & HUD
                        spawn_system_tray();
                        
                        let floating_bar = FridayFloatingBar::new().unwrap();
                        let bar_weak = floating_bar.as_weak();
                        floating_bar.set_is_dark(detect_windows_dark_theme());

                        // State tracking: 0 = Muted, 1 = Live, 2 = PTT
                        let mic_mode = Arc::new(Mutex::new(2)); 
                        let mic_mode_toggle = mic_mode.clone();

                        floating_bar.on_set_mode(move |mode| {
                            let mut current_mode = mic_mode_toggle.lock().unwrap();
                            *current_mode = mode;
                            if let Some(bar) = bar_weak.upgrade() {
                                bar.set_mic_mode(mode);
                                // If switching away from Live, reset listening state
                                if mode != 1 { bar.set_is_listening(false); }
                            }
                        });

                        let _ = floating_bar.show();
                        let _ = ui.hide();

                        spawn_hotkey_listener(floating_bar.as_weak(), mic_mode);
                        Box::leak(Box::new(floating_bar));
                    }
                });
            });
        }
    });

    setup_ui.run()
}

// Background thread listening for True Push-to-Talk (Hold to listen)
fn spawn_hotkey_listener(bar_weak: Weak<FridayFloatingBar>, mic_mode: Arc<Mutex<i32>>) {
    let ctrl_pressed = Arc::new(Mutex::new(false));
    let alt_pressed = Arc::new(Mutex::new(false));
    let is_listening = Arc::new(Mutex::new(false));

    thread::spawn(move || {
        let callback = move |event: Event| {
            let mut ctrl = ctrl_pressed.lock().unwrap();
            let mut alt = alt_pressed.lock().unwrap();

            match event.event_type {
                EventType::KeyPress(Key::ControlLeft) | EventType::KeyPress(Key::ControlRight) => *ctrl = true,
                EventType::KeyRelease(Key::ControlLeft) | EventType::KeyRelease(Key::ControlRight) => *ctrl = false,
                EventType::KeyPress(Key::Alt) | EventType::KeyPress(Key::AltGr) => *alt = true,
                EventType::KeyRelease(Key::Alt) | EventType::KeyRelease(Key::AltGr) => *alt = false,
                _ => {}
            }

            let mode = *mic_mode.lock().unwrap();
            
            // Only trigger hotkeys if in PTT Mode (2)
            if mode == 2 {
                let keys_held = *ctrl && *alt;
                let mut currently_listening = is_listening.lock().unwrap();

                // State change detected
                if keys_held && !*currently_listening {
                    *currently_listening = true;
                    let bar_weak_clone = bar_weak.clone();
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(bar) = bar_weak_clone.upgrade() {
                            bar.set_is_listening(true);
                            // TODO: trigger CPAL audio capture start
                        }
                    });
                } else if !keys_held && *currently_listening {
                    *currently_listening = false;
                    let bar_weak_clone = bar_weak.clone();
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(bar) = bar_weak_clone.upgrade() {
                            bar.set_is_listening(false);
                            // TODO: trigger CPAL audio capture stop
                        }
                    });
                }
            }
        };

        let _ = listen(callback);
    });
}