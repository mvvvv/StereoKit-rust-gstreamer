pub mod video1;
pub mod video2;

use std::sync::Mutex;
use stereokit_rust::{
    event_loop::{SkClosures, StepperAction},
    maths::{units::*, Matrix, Pose, Quat, Vec2, Vec3},
    sk::Sk,
    sprite::Sprite,
    system::{Log, LogLevel, Renderer},
    tex::SHCubemap,
    tools::{
        fly_over::FlyOver,
        log_window::{LogItem, LogWindow},
        os_api::get_external_path,
    },
    ui::{Ui, UiBtnLayout},
    util::{
        named_colors::{BLUE, LIGHT_BLUE, LIGHT_CYAN, WHITE},
        Color128, Gradient,
    },
};
use video1::{gstreamer_init, Video1, VideoType};
use video2::Video2;
use winit::event_loop::EventLoop;

/// Somewhere to copy the log
static LOG_LOG: Mutex<Vec<LogItem>> = Mutex::new(vec![]);

//use crate::launch;
#[cfg(target_os = "android")]
//use android_activity::AndroidApp;
use winit::platform::android::activity::AndroidApp;

// #[cfg(target_os = "android")]
// use jni::{
//     sys::{jint, JNI_VERSION_1_8},
//     JavaVM,
// };
// #[cfg(target_os = "android")]
// use std::os::raw::c_void;
// #[cfg(target_os = "android")]
// #[allow(non_snake_case)]
// #[no_mangle]
// pub unsafe extern "system" fn JNI_OnLoad(vm: JavaVM, _: *mut c_void) -> jint {
//     let env = vm.get_env().expect("Cannot get reference to the JNIEnv");
//     JNI_VERSION_1_8
// }

#[allow(dead_code)]
#[cfg(target_os = "android")]
#[no_mangle]
/// The main function for android app
fn android_main(app: AndroidApp) {
    use stereokit_rust::sk::{DepthMode, OriginMode, SkSettings};
    let mut settings = SkSettings::default();
    settings
        .app_name("rust_gstreamer")
        .assets_folder("assets")
        .origin(OriginMode::Floor)
        .render_multisample(4)
        .render_scaling(2.0)
        .depth_mode(DepthMode::Stencil)
        .log_filter(LogLevel::Diagnostic);

    android_logger::init_once(
        android_logger::Config::default().with_max_level(log::LevelFilter::Debug).with_tag("SKIT_rs_gst"),
    );

    let (sk, event_loop) = settings.init_with_event_loop(app).unwrap();

    _main(sk, event_loop);
}

pub fn _main(sk: Sk, event_loop: EventLoop<StepperAction>) {
    let is_testing = false;
    Log::diag("Launch my_vr_program");
    launch(sk, event_loop, is_testing);
    Sk::shutdown();
}

pub fn launch(mut sk: Sk, event_loop: EventLoop<StepperAction>, _is_testing: bool) {
    Log::diag(
        "======================================================================================================== !!",
    );
    Renderer::scaling(1.5);
    Renderer::multisample(4);

    // We want to be able to view the log using the LogWindow tool
    let fn_mut = |level: LogLevel, log_text: &str| {
        let mut items = LOG_LOG.lock().unwrap();
        for line_text in log_text.lines() {
            let subs = line_text.as_bytes().chunks(130);
            for (pos, sub_line) in subs.enumerate() {
                if let Ok(mut sub_string) = String::from_utf8(sub_line.to_vec()) {
                    if pos > 0 {
                        sub_string.insert_str(0, "‣‣‣‣");
                    }
                    if let Some(item) = items.last_mut() {
                        if item.text == sub_string {
                            item.count += 1;
                            continue;
                        }
                    }

                    items.push(LogItem { level, text: sub_string.to_owned(), count: 1 });
                };
            }
        }
    };

    Log::subscribe(fn_mut);
    // need a way to do that properly Log::unsubscribe(fn_mut);

    let mut log_window = LogWindow::new(&LOG_LOG);
    log_window.pose = Pose::new(Vec3::new(-0.7, 2.0, -0.3), Some(Quat::look_dir(Vec3::new(1.0, 0.0, 1.0))));

    let mut show_log = false;
    log_window.show(show_log);

    sk.push_action(StepperAction::add("LogWindow", log_window));
    sk.push_action(StepperAction::add_default::<FlyOver>("FlyOver"));
    // Open or close the log window
    let event_loop_proxy = sk.get_event_loop_proxy().unwrap();
    let mut send_event_show_log = move || {
        show_log = !show_log;
        let _ = &event_loop_proxy.send_event(StepperAction::event(
            "main".to_string(),
            "ShowLogWindow",
            &show_log.to_string(),
        ));
    };

    // we will have a window to trigger some actions
    let mut window_demo_pose = Pose::new(Vec3::new(-0.7, 1.5, -0.3), Some(Quat::look_dir(Vec3::new(1.0, 0.0, 1.0))));
    let demo_win_width = 60.0 * CM;

    // we create a sky dome to be able to switch from the default sky dome
    let mut gradient_sky = Gradient::new(None);
    gradient_sky
        .add(Color128::BLACK, 0.0)
        .add(BLUE, 0.3)
        .add(LIGHT_BLUE, 0.5)
        .add(LIGHT_CYAN, 0.8)
        .add(WHITE, 1.0);
    let cube0 = SHCubemap::gen_cubemap_gradient(gradient_sky, Vec3::Y, 1024);

    //save the default cubemap.
    let cube_default = SHCubemap::get_rendered_sky();
    cube0.render_as_sky();
    let mut sky = 1;

    //init gstreamer
    if let Err(err) = gstreamer_init() {
        Log::err(format!("Error during gstreamer initialisation : {:?}", err));
    }
    Log::diag(
        "======================================================================================================== !!",
    );
    let radio_on = Sprite::radio_on();
    let radio_off = Sprite::radio_off();

    let mut rtp_stream1 = false;
    //let mut rtp_stream2 = false;
    let mut v3_enabled = false;
    let mut video_h264_dec_active = false;
    let mut video_vp8_dec_active = false;
    let mut video_vp8_https_dec_active = false;
    let mut playbin_h264_active = false;
    let mut video_h264_active = false;
    let mut video_mkv_vp8_active = false;
    SkClosures::run_app(
        sk,
        event_loop,
        |sk, _token| {
            Ui::window_begin("Template", &mut window_demo_pose, Some(Vec2::new(demo_win_width, 0.0)), None, None);
            if Ui::radio_img("Blue light", sky == 1, &radio_off, &radio_on, UiBtnLayout::Left, None) {
                cube0.render_as_sky();
                sky = 1;
            }
            Ui::same_line();
            if Ui::radio_img("Default light", sky == 2, &radio_off, &radio_on, UiBtnLayout::Left, None) {
                cube_default.render_as_sky();
                sky = 2;
            }
            Ui::same_line();
            Ui::hspace(0.25);
            Ui::same_line();
            if let Some(new_value) = Ui::toggle("Show Log", show_log, None) {
                show_log = new_value;
                send_event_show_log();
            }
            Ui::same_line();
            if let Some(new_value) = Ui::toggle("Decodebin3", v3_enabled, None) {
                v3_enabled = new_value;
            }
            Ui::next_line();
            Ui::hseparator();

            if let Some(new_value) = Ui::toggle("RtpStream", rtp_stream1, None) {
                if new_value {
                    // launch rtp stream
                    let mut rtp_stream = Video1::new(VideoType::RtpStream { port: 5000 });
                    rtp_stream.width = 2288;
                    rtp_stream.height = 1430;
                    rtp_stream.transform_screen =
                        Matrix::tr(&(Vec3::new(-0.5, 2.0, -1.5)), &Quat::from_angles(90.0, 0.0, 0.0));
                    sk.push_action(StepperAction::add("RtpStream1", rtp_stream));
                } else {
                    sk.push_action(StepperAction::Remove("RtpStream1".into()));
                }
                rtp_stream1 = new_value;
            }
            // Ui::same_line();
            // if let Some(new_value) = Ui::toggle("RtpRawStream", rtp_stream2, None) {
            //     if new_value {
            //         // launch rtp stream
            //         let mut rtp_raw_stream = Video1::new(VideoType::RtpRawStream { port: 5000 });
            //         rtp_raw_stream.transform_screen =
            //             Matrix::tr(&(Vec3::new(1.5, 2.0, -1.5)), &Quat::from_angles(90.0, 0.0, 0.0));
            //         sk.push_action(StepperAction::add("RtpRawStream", rtp_raw_stream));
            //     } else {
            //         sk.push_action(StepperAction::Remove("RtpRawStream".into()));
            //     }
            //     rtp_stream2 = new_value;
            // }

            Ui::next_line();
            if let Some(new_value) = Ui::toggle("Playbin MP4", playbin_h264_active, None) {
                if new_value {
                    let uri_fmt = if let Some(dir_path) = get_external_path(sk.get_sk_info_clone()) {
                        let file_path = dir_path.join("videos/test.mp4");
                        if file_path.is_file() {
                            Log::diag(format!("File h264 : {:?}", file_path));
                            "file:".to_string() + file_path.to_str().unwrap()
                        } else {
                            Log::warn(format!("No file h264 : {:?}", file_path));
                            "!!!!!!!No File".into()
                        }
                    } else {
                        Log::warn(format!("No external path{}", "!"));
                        "!!!!!!!No external path".into()
                    };
                    // launch video_h264
                    let mut video_h264 = Video2::new(uri_fmt, v3_enabled);
                    video_h264.transform_screen =
                        Matrix::tr(&(Vec3::new(-0.5, 0.8, -1.5)), &Quat::from_angles(90.0, 0.0, 0.0));
                    sk.push_action(StepperAction::add("PlaybinH264", video_h264));
                } else {
                    sk.push_action(StepperAction::Remove("PlaybinH264".into()));
                }
                playbin_h264_active = new_value;
            }

            Ui::next_line();
            if let Some(new_value) = Ui::toggle("Video MP4", video_h264_active, None) {
                if new_value {
                    let uri_fmt = if let Some(dir_path) = get_external_path(sk.get_sk_info_clone()) {
                        let file_path = dir_path.join("videos/test.mp4");
                        if file_path.is_file() {
                            Log::diag(format!("File h264 : {:?}", file_path));
                            file_path.to_str().unwrap().into()
                        } else {
                            Log::warn(format!("No file h264 : {:?}", file_path));
                            "!!!!!!!No File".into()
                        }
                    } else {
                        Log::warn(format!("No external path{}", "!"));
                        "!!!!!!!No external path".into()
                    };
                    // launch video_h264
                    let mut video_h264 = Video1::new(VideoType::H264File { uri: uri_fmt });
                    video_h264.transform_screen =
                        Matrix::tr(&(Vec3::new(-0.5, 0.8, -1.5)), &Quat::from_angles(90.0, 0.0, 0.0));
                    sk.push_action(StepperAction::add("VideoH264", video_h264));
                } else {
                    sk.push_action(StepperAction::Remove("VideoH264".into()));
                }
                video_h264_active = new_value;
            }
            Ui::same_line();
            if let Some(new_value) = Ui::toggle("Video VP8", video_mkv_vp8_active, None) {
                if new_value {
                    let uri_fmt = if let Some(dir_path) = get_external_path(sk.get_sk_info_clone()) {
                        let file_path = dir_path.join("videos/sintel_trailer-480p.webm");
                        if file_path.is_file() {
                            Log::diag(format!("File VP8 : {:?}", file_path));
                            file_path.to_str().unwrap().into()
                        } else {
                            Log::warn(format!("No file VP8 : {:?}", file_path));
                            "!!!!!!No File".into()
                        }
                    } else {
                        Log::warn(format!("No external path{}", "!"));
                        "!!!!!!No external path".into()
                    };
                    // launch video_mkv_vp8
                    let mut video_mkv_vp8 = Video1::new(VideoType::VP8File { uri: uri_fmt });
                    video_mkv_vp8.transform_screen =
                        Matrix::tr(&(Vec3::new(1.5, 0.8, -1.5)), &Quat::from_angles(90.0, 0.0, 0.0));
                    video_mkv_vp8.width = 854;
                    video_mkv_vp8.height = 480;
                    sk.push_action(StepperAction::add("Videomkv_vp8", video_mkv_vp8));
                } else {
                    sk.push_action(StepperAction::Remove("Videomkv_vp8".into()));
                }
                video_mkv_vp8_active = new_value;
            }
            Ui::next_line();
            if let Some(new_value) = Ui::toggle("Video MP4(dec)", video_h264_dec_active, None) {
                if new_value {
                    let uri_fmt = if let Some(dir_path) = get_external_path(sk.get_sk_info_clone()) {
                        let file_path = dir_path.join("videos/test.mp4");
                        if file_path.is_file() {
                            Log::diag(format!("File h264 : {:?}", file_path));
                            format!("file:{}", file_path.to_str().unwrap())
                        } else {
                            Log::warn(format!("No file h264 : {:?}", file_path));
                            "!!!!!!!No File".into()
                        }
                    } else {
                        Log::warn(format!("No external path{}", "!"));
                        "!!!!!!No external path".into()
                    };
                    // launch video_h264
                    let mut video_h264 = Video1::new(VideoType::Decodebin { uri: uri_fmt, v3_enabled });
                    video_h264.transform_screen =
                        Matrix::tr(&(Vec3::new(-0.5, -0.4, -1.5)), &Quat::from_angles(90.0, 0.0, 0.0));
                    sk.push_action(StepperAction::add("VideoH264_dec", video_h264));
                } else {
                    sk.push_action(StepperAction::Remove("VideoH264_dec".into()));
                }
                video_h264_dec_active = new_value;
            }
            Ui::same_line();
            if let Some(new_value) = Ui::toggle("Video VP8(dec)", video_vp8_dec_active, None) {
                if new_value {
                    let uri_fmt = if let Some(dir_path) = get_external_path(sk.get_sk_info_clone()) {
                        let file_path = dir_path.join("videos/sintel_trailer-480p.webm");
                        if file_path.is_file() {
                            Log::diag(format!("File VP8 : {:?}", file_path));
                            format!("file:{}", file_path.to_str().unwrap())
                        } else {
                            Log::warn(format!("No file VP8 : {:?}", file_path));
                            "!!!!!!No File".into()
                        }
                    } else {
                        Log::warn(format!("No external path{}", "!"));
                        "!!!!!!No external path".into()
                    };
                    // launch video_h264
                    let mut video_vp8 = Video1::new(VideoType::Decodebin { uri: uri_fmt, v3_enabled });
                    video_vp8.transform_screen =
                        Matrix::tr(&(Vec3::new(1.5, -0.4, -1.5)), &Quat::from_angles(90.0, 0.0, 0.0));
                    sk.push_action(StepperAction::add("Videovp8_dec", video_vp8));
                } else {
                    sk.push_action(StepperAction::Remove("Videovp8_dec".into()));
                }
                video_vp8_dec_active = new_value;
            }
            Ui::same_line();
            if let Some(new_value) = Ui::toggle("Video VP8 HTTPS(dec)", video_vp8_https_dec_active, None) {
                if new_value {
                    // launch video_vp8
                    let mut video_vp8 = Video1::new(VideoType::Decodebin {
                        uri: "https://gstreamer.freedesktop.org/data/media/sintel_trailer-480p.webm".into(),
                        v3_enabled,
                    });
                    video_vp8.transform_screen =
                        Matrix::tr(&(Vec3::new(3.5, -0.4, -1.5)), &Quat::from_angles(90.0, 0.0, 0.0));
                    sk.push_action(StepperAction::add("video_VP8_dec", video_vp8));
                } else {
                    sk.push_action(StepperAction::Remove("video_VP8_dec".into()));
                }
                video_vp8_https_dec_active = new_value;
            }
            Ui::next_line();
            Ui::hseparator();
            if Ui::button("Exit", Some(Vec2::new(0.10, 0.10))) {
                sk.quit(None);
            }
            Ui::window_end();
        },
        |sk| Log::info(format!("QuitReason is {:?}", sk.get_quit_reason())),
    );
}
