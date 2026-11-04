use sigmaos_display_manager::{Config, DisplayManagerComponent, Component};
fn main() {
    env_logger::init();
    let mut c = DisplayManagerComponent::new(Config::default());
    if let Err(e) = c.initialize() { eprintln!("Failed: {}", e); std::process::exit(1); }
    println!("SigmaOS display-manager running");
    let _ = c.shutdown();
}
