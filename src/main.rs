slint::include_modules!();

#[tokio::main]
async fn main() -> Result<(), slint::PlatformError> {
    // 1. Initialize the Setup UI
    let setup_ui = FridaySetup::new()?;
    let setup_weak = setup_ui.as_weak();

    // 2. Handle the "Get Started" button click
    setup_ui.on_initialize(move |gemini_key, eleven_key| {
        println!("Keys Captured!");
        println!("Gemini Key Length: {}", gemini_key.len());
        println!("ElevenLabs Key Length: {}", eleven_key.len());
        
        // TODO: Pass keys to the Tokio Audio/LLM pipeline here.

        // 3. Launch the Floating Status Bar
        let floating_bar = FridayFloatingBar::new().unwrap();
        
        floating_bar.on_trigger_mic(|| {
            println!("Mic triggered! Capturing audio...");
            // TODO: Trigger CPAL audio capture here
        });

        floating_bar.show().unwrap();

        // 4. Hide the main setup window
        if let Some(ui) = setup_weak.upgrade() {
            ui.hide().unwrap();
        }

        // Keep the floating bar alive in memory
        Box::leak(Box::new(floating_bar));
    });

    // Run the UI loop
    setup_ui.run()
}