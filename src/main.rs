#![windows_subsystem = "windows"]

mod tools;
mod audio;
mod ai;

slint::include_modules!();
use ai::FridayBrain;
use audio::AudioRecorder;
use rdev::{listen, Event, EventType, Key};
use reqwest::Client;
use std::fs;
use std::sync::{Arc, Mutex};
use std::thread;
use slint::{ComponentHandle, SharedString, Weak};
use tray_icon::{TrayIconBuilder, menu::{Menu, MenuItem, MenuEvent}, Icon};
use tokio::runtime::Handle;

const ENV_FILE: &str = ".env";

#[cfg(target_os = "windows")]
fn enable_dpi_awareness() {
    use windows_sys::Win32::UI::WindowsAndMessaging::SetProcessDPIAware;
    unsafe {
        SetProcessDPIAware();
    }
}

#[cfg(target_os = "windows")]
fn configure_hud_window(title_str: &str, window_y: i32) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        FindWindowW, GetWindowLongW, SetWindowLongW, SetWindowPos, GetSystemMetrics, GetWindowRect,
        ShowWindow, GWL_EXSTYLE, WS_EX_LAYERED, WS_EX_TOOLWINDOW, WS_EX_APPWINDOW, WS_EX_TOPMOST,
        SM_CXSCREEN, HWND_TOPMOST, SWP_NOACTIVATE, SWP_SHOWWINDOW, SWP_FRAMECHANGED, SW_HIDE, SW_SHOW
    };
    use windows_sys::Win32::Foundation::RECT;
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;

    let title: Vec<u16> = OsStr::new(&format!("{}\0", title_str)).encode_wide().collect();

    for _ in 0..20 {
        unsafe {
            let hwnd = FindWindowW(std::ptr::null(), title.as_ptr());
            if hwnd != 0 {
                ShowWindow(hwnd, SW_HIDE);

                let mut ex_style = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32;
                ex_style &= !WS_EX_APPWINDOW;
                ex_style |= WS_EX_TOOLWINDOW | WS_EX_LAYERED | WS_EX_TOPMOST;
                SetWindowLongW(hwnd, GWL_EXSTYLE, ex_style as i32);

                ShowWindow(hwnd, SW_SHOW);

                let mut rect: RECT = std::mem::zeroed();
                if GetWindowRect(hwnd, &mut rect) != 0 {
                    let actual_width = rect.right - rect.left;
                    let actual_height = rect.bottom - rect.top;

                    if actual_width > 50 {
                        let screen_width = GetSystemMetrics(SM_CXSCREEN);
                        let pos_x = (screen_width - actual_width) / 2;
                        SetWindowPos(
                            hwnd,
                            HWND_TOPMOST,
                            pos_x,
                            window_y,
                            actual_width,
                            actual_height,
                            SWP_NOACTIVATE | SWP_SHOWWINDOW | SWP_FRAMECHANGED,
                        );
                        break;
                    }
                }
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}

fn detect_windows_dark_theme() -> bool {
    #[cfg(target_os = "windows")]
    {
        use std::process::Command;
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;

        let output = Command::new("powershell")
            .creation_flags(CREATE_NO_WINDOW)
            .args(["-NoProfile", "-NonInteractive", "-Command", "Get-ItemPropertyValue -Path 'HKCU:\\Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize' -Name AppsUseLightTheme -ErrorAction SilentlyContinue"])
            .output();

        if let Ok(out) = output {
            let res = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if res == "1" { return false; }
        }
    }
    true
}

fn load_saved_credentials() -> (Option<String>, Option<String>) {
    let _ = dotenv::dotenv();
    let gemini = std::env::var("GEMINI_API_KEY").ok().filter(|s| !s.trim().is_empty());
    let eleven = std::env::var("ELEVENLABS_API_KEY").ok().filter(|s| !s.trim().is_empty());
    (gemini, eleven)
}

fn save_credentials(gemini_key: &str, eleven_key: &str) {
    let contents = format!(
        "GEMINI_API_KEY={}\nELEVENLABS_API_KEY={}\n",
        gemini_key.trim(),
        eleven_key.trim()
    );
    let _ = fs::write(ENV_FILE, contents);
}

async fn validate_gemini_key(client: &Client, key: &str) -> bool {
    let url = format!("https://generativelanguage.googleapis.com/v1beta/models?key={}", key);
    match client.get(&url).send().await {
        Ok(res) => res.status().is_success(),
        Err(_) => false,
    }
}

async fn validate_elevenlabs_key(client: &Client, key: &str) -> bool {
    let url = "https://api.elevenlabs.io/v1/user";
    match client.get(url).header("xi-api-key", key).send().await {
        Ok(res) => res.status().is_success(),
        Err(_) => false,
    }
}

fn spawn_system_tray() {
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

    thread::spawn(move || {
        loop {
            if let Ok(event) = MenuEvent::receiver().try_recv() {
                if event.id == quit_id { std::process::exit(0); }
            }
            thread::sleep(std::time::Duration::from_millis(100));
        }
    });
    Box::leak(Box::new(tray_icon)); 
}

fn initialize_hud(gemini_key: String, eleven_key: Option<String>, rt_handle: Handle) {
    spawn_system_tray();
    let floating_bar = FridayFloatingBar::new().unwrap();
    let bar_weak = floating_bar.as_weak();
    floating_bar.set_is_dark(detect_windows_dark_theme());

    let mic_mode = Arc::new(Mutex::new(2)); 
    let mic_mode_toggle = mic_mode.clone();

    floating_bar.on_set_mode(move |mode| {
        let mut current_mode = mic_mode_toggle.lock().unwrap();
        *current_mode = mode;
        if let Some(bar) = bar_weak.upgrade() {
            bar.set_mic_mode(mode);
            if mode != 1 { bar.set_is_listening(false); }
        }
    });

    let _ = floating_bar.show();

    #[cfg(target_os = "windows")]
    {
        std::thread::spawn(move || {
            configure_hud_window("F.R.I.D.A.Y. HUD", 12);
        });
    }

    let brain = Arc::new(FridayBrain::new(gemini_key, eleven_key));
    let recorder = Arc::new(AudioRecorder::new());

    spawn_hotkey_listener(floating_bar.as_weak(), mic_mode, recorder, brain, rt_handle);
    Box::leak(Box::new(floating_bar));
}

#[tokio::main]
async fn main() -> Result<(), slint::PlatformError> {
    #[cfg(target_os = "windows")]
    enable_dpi_awareness();

    let is_dark_system = detect_windows_dark_theme();
    let http_client = Client::new();
    let rt_handle = Handle::current();

    let (saved_gemini, saved_eleven) = load_saved_credentials();

    if let Some(g_key) = saved_gemini {
        if validate_gemini_key(&http_client, &g_key).await {
            initialize_hud(g_key, saved_eleven, rt_handle);
            slint::run_event_loop()?;
            return Ok(());
        }
    }

    let setup_ui = FridaySetup::new()?;
    let setup_weak = setup_ui.as_weak();
    setup_ui.set_is_dark(is_dark_system);

    setup_ui.window().set_fullscreen(true);

    setup_ui.on_minimize_app({
        let setup_weak = setup_weak.clone();
        move || { 
            if let Some(ui) = setup_weak.upgrade() { 
                ui.window().set_minimized(true); 
            } 
        }
    });

    setup_ui.on_exit_app(|| { std::process::exit(0); });

    setup_ui.on_initialize({
        let setup_weak = setup_weak.clone();
        let http_client = http_client.clone();
        let rt_handle = rt_handle.clone();

        move |gemini_key, eleven_key| {
            let g_key = gemini_key.trim().to_string();
            let e_key = eleven_key.trim().to_string();
            let setup_weak = setup_weak.clone();
            let http_client = http_client.clone();
            let rt_handle = rt_handle.clone();

            if g_key.is_empty() {
                if let Some(ui) = setup_weak.upgrade() { 
                    ui.set_error_msg(SharedString::from("Gemini API key is required.")); 
                }
                return;
            }

            if let Some(ui) = setup_weak.upgrade() {
                ui.set_is_validating(true);
                ui.set_error_msg(SharedString::from(""));
            }

            tokio::spawn(async move {
                let is_gemini_valid = validate_gemini_key(&http_client, &g_key).await;
                if !is_gemini_valid {
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(ui) = setup_weak.upgrade() {
                            ui.set_is_validating(false);
                            ui.set_error_msg(SharedString::from("Invalid Gemini API key."));
                        }
                    });
                    return;
                }

                let optional_eleven = if !e_key.is_empty() {
                    let is_eleven_valid = validate_elevenlabs_key(&http_client, &e_key).await;
                    if !is_eleven_valid {
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(ui) = setup_weak.upgrade() {
                                ui.set_is_validating(false);
                                ui.set_error_msg(SharedString::from("Invalid ElevenLabs API key."));
                            }
                        });
                        return;
                    }
                    Some(e_key.clone())
                } else {
                    None
                };

                save_credentials(&g_key, &e_key);

                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(ui) = setup_weak.upgrade() {
                        ui.set_is_validating(false);
                        initialize_hud(g_key, optional_eleven, rt_handle);
                        let _ = ui.hide();
                    }
                });
            });
        }
    });

    setup_ui.run()
}

fn spawn_hotkey_listener(
    bar_weak: Weak<FridayFloatingBar>,
    mic_mode: Arc<Mutex<i32>>,
    recorder: Arc<AudioRecorder>,
    brain: Arc<FridayBrain>,
    rt_handle: Handle,
) {
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
            
            if mode == 2 {
                let keys_held = *ctrl && *alt;
                let mut currently_listening = is_listening.lock().unwrap();

                if keys_held && !*currently_listening {
                    *currently_listening = true;
                    let _ = recorder.start_recording();
                    let bar_weak_clone = bar_weak.clone();
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(bar) = bar_weak_clone.upgrade() {
                            bar.set_is_listening(true);
                        }
                    });
                } else if !keys_held && *currently_listening {
                    *currently_listening = false;
                    let samples = recorder.stop_recording();

                    let bar_weak_clone = bar_weak.clone();
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(bar) = bar_weak_clone.upgrade() {
                            bar.set_is_listening(false);
                        }
                    });

                    let brain_clone = brain.clone();
                    rt_handle.spawn(async move {
                        let _ = brain_clone.process_voice_input(samples).await;
                    });
                }
            }
        };
        let _ = listen(callback);
    });
}