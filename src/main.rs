#![windows_subsystem = "windows"]

slint::include_modules!();
use rdev::{listen, Event, EventType, Key};
use reqwest::Client;
use std::sync::{Arc, Mutex};
use std::thread;
use slint::{ComponentHandle, SharedString, Weak};

// Check Windows System Theme (Registry query or fallback)
fn detect_windows_dark_theme() -> bool {
    #[cfg(target_os = "windows")]
    {
        use std::process::Command;
        let output = Command::new("powershell")
            .args(["-Command", "Get-ItemPropertyValue -Path 'HKCU:\\Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize' -Name AppsUseLightTheme -ErrorAction SilentlyContinue"])
            .output();

        if let Ok(out) = output {
            let res = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if res == "1" {
                return false; // Light theme enabled
            }
        }
    }
    true // Default to dark theme
}

// Perform handshake validation with Gemini API
async fn validate_gemini_key(client: &Client, key: &str) -> bool {
    let url = format!(
        "https://generativelanguage.googleapis.com/v1beta/models?key={}",
        key
    );
    match client.get(&url).send().await {
        Ok(res) => res.status().is_success(),
        Err(_) => false,
    }
}

#[tokio::main]
async fn main() -> Result<(), slint::PlatformError> {
    let is_dark_system = detect_windows_dark_theme();
    let http_client = Client::new();

    let setup_ui = FridaySetup::new()?;
    let setup_weak = setup_ui.as_weak();
    setup_ui.set_is_dark(is_dark_system);

    // Dismiss window when clicking background
    setup_ui.on_dismiss_window({
        let setup_weak = setup_weak.clone();
        move || {
            if let Some(ui) = setup_weak.upgrade() {
                let _ = ui.hide();
            }
        }
    });

    // Exit application
    setup_ui.on_exit_app(|| {
        std::process::exit(0);
    });

    // Key Validation and Initialization
    setup_ui.on_initialize({
        let setup_weak = setup_weak.clone();
        let http_client = http_client.clone();

        move |gemini_key, eleven_key| {
            let key = gemini_key.trim().to_string();
            let _eleven = eleven_key.trim().to_string();
            let setup_weak = setup_weak.clone();
            let http_client = http_client.clone();

            if key.is_empty() {
                if let Some(ui) = setup_weak.upgrade() {
                    ui.set_error_msg(SharedString::from("Gemini API key is required."));
                }
                return;
            }

            if let Some(ui) = setup_weak.upgrade() {
                ui.set_is_validating(true);
                ui.set_error_msg(SharedString::from(""));
            }

            // Spawn async verification task
            tokio::spawn(async move {
                let is_valid = validate_gemini_key(&http_client, &key).await;

                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(ui) = setup_weak.upgrade() {
                        ui.set_is_validating(false);

                        if !is_valid {
                            ui.set_error_msg(SharedString::from("Invalid API key or network error."));
                            return;
                        }

                        // Success -> Spawn Floating HUD
                        let floating_bar = FridayFloatingBar::new().unwrap();
                        let bar_weak = floating_bar.as_weak();
                        floating_bar.set_is_dark(detect_windows_dark_theme());

                        // Floating Bar Event Handlers
                        floating_bar.on_exit_app(|| {
                            std::process::exit(0);
                        });

                        let is_muted = Arc::new(Mutex::new(false));
                        let is_muted_toggle = is_muted.clone();
                        let bar_weak_toggle = bar_weak.clone();

                        floating_bar.on_toggle_mic(move || {
                            let mut muted = is_muted_toggle.lock().unwrap();
                            *muted = !*muted;
                            if let Some(bar) = bar_weak_toggle.upgrade() {
                                bar.set_is_muted(*muted);
                            }
                        });

                        let _ = floating_bar.show();
                        let _ = ui.hide();

                        // Launch Global Hotkey Worker
                        spawn_hotkey_listener(bar_weak, is_muted);

                        Box::leak(Box::new(floating_bar));
                    }
                });
            });
        }
    });

    setup_ui.run()
}

// Background thread listening globally for Ctrl + Alt
fn spawn_hotkey_listener(bar_weak: Weak<FridayFloatingBar>, is_muted: Arc<Mutex<bool>>) {
    let ctrl_pressed = Arc::new(Mutex::new(false));
    let alt_pressed = Arc::new(Mutex::new(false));
    let is_listening = Arc::new(Mutex::new(false));

    thread::spawn(move || {
        let callback = move |event: Event| {
            let mut ctrl = ctrl_pressed.lock().unwrap();
            let mut alt = alt_pressed.lock().unwrap();
            let mut listening = is_listening.lock().unwrap();

            match event.event_type {
                EventType::KeyPress(Key::ControlLeft) | EventType::KeyPress(Key::ControlRight) => *ctrl = true,
                EventType::KeyRelease(Key::ControlLeft) | EventType::KeyRelease(Key::ControlRight) => *ctrl = false,
                EventType::KeyPress(Key::Alt) | EventType::KeyPress(Key::AltGr) => *alt = true,
                EventType::KeyRelease(Key::Alt) | EventType::KeyRelease(Key::AltGr) => *alt = false,
                _ => {}
            }

            if *ctrl && *alt {
                let muted = *is_muted.lock().unwrap();
                if !muted {
                    *listening = !*listening;
                    let state = *listening;
                    let bar_weak_clone = bar_weak.clone();

                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(bar) = bar_weak_clone.upgrade() {
                            if state {
                                bar.set_status_text(SharedString::from("Listening..."));
                            } else {
                                bar.set_status_text(SharedString::from("Standby (Ctrl+Alt)"));
                            }
                        }
                    });
                }
                *ctrl = false;
                *alt = false;
            }
        };

        let _ = listen(callback);
    });
}