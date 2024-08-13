use std::{
    env::{self, current_dir},
    process::Command,
};

macro_rules! cargo_link {
    ($feature:expr) => {
        println!("cargo:rustc-link-lib={}", $feature);
    };
}

fn main() {
    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap();
    let target_family = env::var("CARGO_CFG_TARGET_FAMILY").unwrap();
    let build_os = match env::consts::OS {
        "linux" => "linux",
        "windows" => "windows",
        _ => panic!("Unsupported OS. You must use either Linux, MacOS or Windows to build the crate."),
    };

    match target_family.as_str() {
        "windows" => {}
        "wasm" => {}
        "unix" => {
            if target_os == "macos" {
                panic!("Sorry, macos is not supported for stereokit.");
            }
            if target_os == "android" {
                let android_ndk_home = env::var("ANDROID_NDK_ROOT").expect("ANDROID_NDK_ROOT not set");
                let gst_libs = env::var("GSTREAMER_PATH").expect("GSTREAMER_PATH not set");
                let gst_libs_path = current_dir().unwrap().join(&gst_libs);

                //make creates a lot of dirs so we move to target/android_build/
                let gst_android_build_path = "target/gst-android-build";
                if let Err(_e) = std::fs::create_dir("target") {};
                if let Err(_e) = std::fs::create_dir(gst_android_build_path) {};
                env::set_current_dir("target").expect("Unable to get a build directory for android gstreamer");

                Command::new("make")
                    .env("BUILD_SYSTEM", format!("{}/build/core", android_ndk_home))
                    .env("GSTREAMER_JAVA_SRC_DIR", "../../app/src/main/java")
                    .env("GSTREAMER_ASSETS_DIR", "../../assets")
                    .env("GSTREAMER_ROOT_ANDROID", gst_libs_path.to_str().unwrap())
                    .env(
                        "GSTREAMER_NDK_BUILD_PATH",
                        gst_libs_path.join("share/gst-android/ndk-build/").to_str().unwrap(),
                    )
                    .args(["-f", &format!("{}/build/core/build-local.mk", android_ndk_home)])
                    .status()
                    .expect("failed to make!");

                env::set_current_dir("../..").expect("Unable to get the right working directory");

                println!("cargo:rustc-link-search=native={}/arm64-v8a", gst_android_build_path);
                cargo_link!("gstreamer_android");
                cargo_link!("dylib=c++");

                println!("cargo:rustc-link-search=native={}/lib", gst_libs);

                cargo_link!("ffi");
                cargo_link!("iconv");
                cargo_link!("intl");
                cargo_link!("orc-0.4");
                cargo_link!("gstreamer-1.0");
                cargo_link!("gmodule-2.0");
                cargo_link!("gobject-2.0");
                cargo_link!("glib-2.0");
                cargo_link!("pcre2-8");
                cargo_link!("gstvideo-1.0");
                cargo_link!("gstaudio-1.0");
                cargo_link!("gstapp-1.0");

                const DEFAULT_CLANG_VERSION: &str = "14.0.7";
                let clang_version = env::var("NDK_CLANG_VERSION").unwrap_or_else(|_| DEFAULT_CLANG_VERSION.to_owned());
                let linux_x86_64_lib_dir =
                    format!("toolchains/llvm/prebuilt/{build_os}-x86_64/lib64/clang/{clang_version}/lib/linux/");
                println!("cargo:rustc-link-search={android_ndk_home}/{linux_x86_64_lib_dir}");
                cargo_link!(format!("clang_rt.builtins-aarch64-android"));
            }
        }
        _ => {
            panic!("target family is unknown");
        }
    }
    // rerun if necessary
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=jni/Android.mk");
}
