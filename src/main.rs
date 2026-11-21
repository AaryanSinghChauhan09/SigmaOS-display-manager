//! SigmaOS-display-manager — display manager CLI.
//!
//! Usage:
//!   sigmaos-disp list | connected | backlights
//!   sigmaos-disp bright <output|backlight> <0.0-1.0>
//!   sigmaos-disp info <output>
//!
//! Mode/position/rotation need compositor/DRM — reported honestly.

use display_manager::{DisplayManager, DispBackend, HostBackend};
use std::process::exit;

fn usage() -> ! {
    eprintln!("usage: sigmaos-disp <list|connected|backlights|bright|info> [args]");
    exit(2);
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() { usage(); }
    let be = HostBackend;
    let cmd = args[0].as_str();
    let rest = &args[1..];

    let result: Result<(), String> = (|| -> Result<(), String> { match cmd {
        "list" => {
            let outs = be.outputs()?;
            print!("{}", DisplayManager::render(&outs));
            Ok(())
        }
        "connected" => {
            let outs = DisplayManager::connected(&be)?;
            print!("{}", DisplayManager::render(&outs));
            Ok(())
        }
        "backlights" => {
            for (name, max) in be.backlights()? {
                println!("{:<30} max={}", name, max);
            }
            Ok(())
        }
        "bright" => {
            if rest.len() < 2 { return Err("bright <name> <level>".into()); }
            let level: f32 = rest[1].parse().map_err(|_| "bad level")?;
            DisplayManager::set_brightness(&be, &rest[0], level).map_err(|e| e.to_string())?;
            println!("brightness {} = {:.0}%", rest[0], level*100.0);
            Ok(())
        }
        "info" => {
            let name = rest.get(0).map(|s| s.as_str()).unwrap_or("");
            let o = DisplayManager::output(&be, name).map_err(|e| e.to_string())?;
            println!("name: {}", o.name);
            println!("connected: {}", o.connected);
            println!("manufacturer: {}", o.manufacturer);
            println!("model: {}", o.model);
            println!("serial: {}", o.serial);
            println!("size: {}x{}mm", o.width_mm, o.height_mm);
            println!("modes: {}", o.modes.len());
            for m in &o.modes { println!("  {}", m.label()); }
            Ok(())
        }
        _ => usage(),
    } })();

    match result {
        Ok(()) => {}
        Err(e) => { eprintln!("sigmaos-disp: {}", e); exit(1); }
    }
}
