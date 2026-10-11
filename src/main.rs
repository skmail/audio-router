#![deny(unsafe_op_in_unsafe_fn)]
#[cfg(not(target_os = "macos"))]
compile_error!("Audio Router is a macOS-only application.");
mod apps;
mod audio;
mod state;
mod ui;
use objc2::{MainThreadMarker, rc::autoreleasepool, runtime::ProtocolObject};
use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy};

fn main() {
    autoreleasepool(|_| {
        let mtm = MainThreadMarker::new().expect("Audio Router must start on the main thread");
        if std::env::args().any(|a| a == "--list") {
            match (audio::devices(), apps::discover()) {
                (Ok(devices), Ok(apps)) => {
                    println!("OUTPUTS");
                    for device in devices {
                        println!("{}  [{}]  {} Hz", device.name, device.uid, device.rate);
                    }
                    println!("\nAUDIO APPS");
                    for app in apps {
                        println!(
                            "{}  [{}]  playing={}  processes={:?}",
                            app.name, app.key, app.playing, app.processes
                        );
                    }
                }
                (Err(error), _) | (_, Err(error)) => {
                    eprintln!("{error}");
                    std::process::exit(1);
                }
            }
            return;
        }
        let app = NSApplication::sharedApplication(mtm);
        app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
        let delegate = ui::Delegate::new(mtm);
        app.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
        app.run();
    });
}
