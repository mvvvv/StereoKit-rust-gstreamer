#[cfg(not(target_os = "android"))]
use std::env;

#[cfg(not(target_os = "android"))]
use stereokit_rust::{
    sk::{AppMode, OriginMode, SkSettings},
    system::LogLevel,
};

pub const USAGE: &str = r#"Usage : program [OPTION] 
    launch Stereokit tests and demos
    
        --test              : test mode
        --headless          : no display at all for --test
        --help              : help"#;

#[allow(dead_code)]
#[cfg(not(target_os = "android"))]
/// The main function when launched on PC. Set --test to use the simulator
fn main() {
    use stereokit_rust::sk::Sk;
    use stereokit_rust_gstreamer::launch;

    let mut headless = false;
    let mut is_testing = false;
    let args = env::args().skip(1);
    for arg in args {
        match &arg[..] {
            "--headless" => headless = true,
            "--test" => is_testing = true,
            "--help" => println!("{}", USAGE),
            _ => {
                if arg.starts_with('-') {
                    println!("Unkown argument {}", arg);
                } else {
                    println!("Unkown positional argument {}", arg);
                }
                println!("{}", USAGE);
            }
        }
    }
    let mut settings = SkSettings::default();
    settings
        .app_name("rust_gstreamer")
        .assets_folder("assets")
        .origin(OriginMode::Stage)
        .log_filter(LogLevel::Diagnostic)
        .no_flatscreen_fallback(true);

    if is_testing {
        if headless {
            settings.mode(AppMode::Offscreen);
        } else {
            settings.mode(AppMode::Simulator);
        }
        settings.disable_unfocused_sleep(true);
    }

    let (sk, event_loop) = settings.init_with_event_loop().unwrap();
    launch(sk, event_loop, is_testing);
    Sk::shutdown();
}

#[allow(dead_code)]
#[cfg(target_os = "android")]
//fake main fn for android because it will use lib.rs/android_main(...)
fn main() {}
