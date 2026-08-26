// THIS LINE KILLS THE UNPROFESSIONAL BLACK BACKGROUND TERMINAL
#![windows_subsystem = "windows"]

slint::include_modules!();
use rdev::{listen, Event, EventType, Key};
use std::sync::{Arc, Mutex};
use std::thread;
use slint::Weak;

#[tokio::main]
async fn main() -> Result<(), slint::PlatformError> {
    let setup_ui = FridaySetup::new()?;
    let setup_weak = setup_ui.as_weak();

    setup_ui.on_close_window(move || {
        std::process::exit(0);
    });

    setup_ui.on_initialize(move |gemini_key, eleven_key| {
        // 1. Launch the Floating Status Bar
        let floating_bar = FridayFloatingBar::new().unwrap();
        let bar_weak = floating_bar.as_weak();
        
        floating_bar.on_trigger_mic(|| {
            // Manual click toggle
            println!("Mic toggled via click!");
        });

        floating_bar.show().unwrap();

        // 2. Hide the main setup window
        if let Some(ui) = setup_weak.upgrade() {
            ui.hide().unwrap();
        }

        // 3. Spawn the Global Hotkey Listener (Ctrl + Alt)
        spawn_hotkey_listener(bar_weak);

        // Keep the floating bar alive
        Box::leak(Box::new(floating_bar));
    });

    setup_ui.run()
}

// Background thread to listen for Ctrl + Alt globally
fn spawn_hotkey_listener(bar_weak: Weak<FridayFloatingBar>) {
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

            // If BOTH are pressed, toggle the UI state
            if *ctrl && *alt {
                *listening = !*listening; // Flip the state
                let current_state = *listening;
                
                // Update the Slint UI thread safely
                let bar_weak_clone = bar_weak.clone();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(bar) = bar_weak_clone.upgrade() {
                        if current_state {
                            bar.set_status(slint::SharedString::from("Listening..."));
                            // TODO: ACTUALLY START RECORDING AUDIO HERE
                        } else {
                            bar.set_status(slint::SharedString::from("Standby"));
                            // TODO: STOP RECORDING AND SEND TO GEMINI HERE
                        }
                    }
                });
                
                // Prevent rapid-fire toggling by forcing them to release keys
                *ctrl = false;
                *alt = false;
            }
        };

        if let Err(error) = listen(callback) {
            println!("Error listening to global keystrokes: {:?}", error);
        }
    });
}