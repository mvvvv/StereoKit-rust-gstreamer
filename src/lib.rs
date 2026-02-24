pub mod video1;

use std::sync::Mutex;

// Common imports (used on all platforms)
use stereokit_rust::{
    framework::{SkClosures, StepperAction},
    maths::{Pose, Quat, Vec2, Vec3, units::*},
    sk::Sk,
    sprite::Sprite,
    system::{Log, LogItem, LogLevel, Renderer},
    tex::SHCubemap,
    tools::{
        fly_over::FlyOver,
        log_window::{basic_log_fmt, LogWindow, SHOW_LOG_WINDOW},
        os_api::get_external_path,
    },
    ui::{Ui, UiBtnLayout},
    util::{
        Color128, Gradient, named_colors::{BLUE, LIGHT_BLUE, LIGHT_CYAN, WHITE}
    },
};
use video1::{gstreamer_init, Coding, Video1, VideoType};
use winit::event_loop::EventLoop;
// Android-specific imports
#[cfg(target_os = "android")]
use winit::platform::android::activity::AndroidApp;

/// Somewhere to copy the log
static LOG_LOG: Mutex<Vec<LogItem>> = Mutex::new(vec![]);



#[allow(dead_code)]
#[cfg(target_os = "android")]
#[no_mangle]
/// The main function for android app
fn android_main(app: AndroidApp) {
    use stereokit_rust::sk::{DepthMode, OriginMode, SkSettings};
    use stereokit_rust::system::{LogLevel, BackendOpenXR};
    let mut settings = SkSettings::default();
    settings
        .app_name("rust_gstreamer")
        .origin(OriginMode::Floor)
        .render_multisample(4)
        .render_scaling(1.5)
        .depth_mode(DepthMode::D32)
        .log_filter(LogLevel::Diagnostic);

    android_logger::init_once(
        android_logger::Config::default().with_max_level(log::LevelFilter::Debug).with_tag("SKIT_rs_gst"),
    );
    
    BackendOpenXR::request_ext("XR_KHR_android_surface_swapchain");
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
    Renderer::scaling(2.0);
    Renderer::multisample(4);

    // Sending formated log to our mutex for the log window.
    let fn_mut = |level: LogLevel, log_text: &str| {
        let items = LOG_LOG.lock().unwrap();
        basic_log_fmt(level, log_text, 120, items);
    };
    Log::subscribe(fn_mut);
    // need a way to do that properly Log::unsubscribe(fn_mut);
    let mut show_log = false;  // The log window is hidden at start
    let mut log_window = LogWindow::new(&LOG_LOG);
    log_window.window_pose = Pose::new(Vec3::new(-0.7, 2.0, -0.3), Some(Quat::look_dir(Vec3::new(1.0, 0.0, 1.0))));
    log_window.enabled = show_log;
    sk.send_event(StepperAction::add("LogWindow", log_window));

    // Fly over to navigate in the scene
    sk.send_event(StepperAction::add_default::<FlyOver>("FlyOver"));


    // we will have a window to trigger some actions
    let mut window_demo_pose = Pose::new(Vec3::new(-0.7, 1.5, -0.3), Some(Quat::look_dir(Vec3::new(1.0, 0.0, 1.0))));
    let demo_win_width = 80.0 * CM;

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
    cube_default.render_as_sky();
    let mut sky = 2;
    let mut show_sky = true;
    let default_clear_color = Renderer::get_clear_color();
    let hidden_sky_clear_color = LIGHT_CYAN;
    Renderer::enable_sky(show_sky);

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
    let mut coding = Coding::H264;
    let mut rtp_stream_decodebin = false;

    let mut rtp_stream_native = false;
    let mut rtp_stream_uri_playbin = false;
    let mut rtsp_server_playbin = false;

    let mut video_h265_dec_active = false;
    let mut video_h264_dec_active = false;
    let mut video_mkv_vp8_dec_active = false;

    let mut video_h264_play_active = false;
    let mut video_vp8_play_active = false;
    let mut video_vp8_https_play_active = false;
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
            if let Some(new_value) = Ui::toggle("Show Sky", &mut show_sky, None) {
                Renderer::enable_sky(new_value);
                if new_value {
                    Renderer::clear_color(default_clear_color);
                } else {
                    Renderer::clear_color(hidden_sky_clear_color);
                }
            }
            Ui::same_line();
            Ui::hspace(0.25);
            Ui::same_line();
            if let Some(new_value) = Ui::toggle("Show Log", &mut show_log, None) {
                sk.send_event(StepperAction::Event("LogWindow".into(), SHOW_LOG_WINDOW.into(), new_value.to_string()));
            }
            Ui::same_line();
            Ui::next_line();
            Ui::hseparator();
            if Ui::radio_img("H264", coding == Coding::H264, &radio_off, &radio_on, UiBtnLayout::Left, None) {
                coding = Coding::H264;
            }
            Ui::same_line();
            if Ui::radio_img("H265", coding == Coding::H265, &radio_off, &radio_on, UiBtnLayout::Left, None) {
                coding = Coding::H265;
            }
            Ui::same_line();
            if Ui::radio_img("VP9", coding == Coding::VP9, &radio_off, &radio_on, UiBtnLayout::Left, None) {
                coding = Coding::VP9;
            }
            Ui::next_line();
            if let Some(new_value) = Ui::toggle("Rtp Stream (low latency)", &mut rtp_stream1, None) {
                if new_value {
                    // launch rtp stream
                    let mut rtp_stream = Video1::new(VideoType::RtpStream { port: 5000, coding: coding.clone() });
                    rtp_stream.width = 3840;
                    rtp_stream.height = 2160;
                    rtp_stream.screen_pose =
                        Pose::new(Vec3::new(-0.5, 2.0, -1.5), Some(Quat::from_angles(90.0, 0.0, 0.0)));
                    sk.send_event(StepperAction::add("RtpStream", rtp_stream));
                } else {
                    sk.send_event(StepperAction::Remove("RtpStream".into()));
                }
            }
            Ui::same_line();
            if let Some(new_value) = Ui::toggle("Rtp Stream Decodebin (low latency)", &mut rtp_stream_decodebin, None) {
                if new_value {
                    // launch rtp stream
                    let mut rtp_stream_decodebin =
                        Video1::new(VideoType::RtpStreamDecodebin { port: 5000, coding: coding.clone() });
                    rtp_stream_decodebin.screen_pose =
                        Pose::new(Vec3::new(1.5, 2.0, -1.5), Some(Quat::from_angles(90.0, 0.0, 0.0)));
                    sk.send_event(StepperAction::add("RtpStreamDecodebin", rtp_stream_decodebin));
                } else {
                    sk.send_event(StepperAction::Remove("RtpStreamDecodebin".into()));
                }
            }
            Ui::same_line();
            if let Some(new_value) = Ui::toggle("Rtp Stream Native (VideoOverlay)", &mut rtp_stream_native, None) {
                if new_value {
                    // Launch platform native overlay pipeline
                    let mut rtp_stream_native =
                        Video1::new(VideoType::RtpStreamNative { port: 5000, coding: coding.clone() });
                    rtp_stream_native.screen_pose =
                        Pose::new(Vec3::new(2.5, 2.0, -1.5), Some(Quat::from_angles(90.0, 0.0, 0.0)));
                    sk.send_event(StepperAction::add("RtpStreamNative", rtp_stream_native));
                } else {
                    sk.send_event(StepperAction::Remove("RtpStreamNative".into()));
                }
            }
            Ui::next_line();
            if let Some(new_value) = Ui::toggle("RTP Stream Playbin", &mut rtp_stream_uri_playbin, None) {
                if new_value {
                    
                    let uri_fmt = match coding { 
                        Coding::H264 => "rtp://0.0.0.0:5000?media=video&clock-rate=90000&encoding-name=h264&payload=96&latency-ms=50&rtp-profile=1".to_string(),
                        Coding::H265 => "rtp://0.0.0.0:5000?media=video&clock-rate=90000&encoding-name=h265&payload=96&latency-ms=50&rtp-profile=1".to_string(),
                        Coding::VP9 => "rtp://0.0.0.0:5000?media=video&clock-rate=90000&encoding-name=vp9&payload=96&latency-ms=50&rtp-profile=1".to_string(),
                    };
                    let mut rtp_stream_uri_playbin_s = Video1::new(VideoType::UriPlaybin { uri: uri_fmt });
                    rtp_stream_uri_playbin_s.screen_pose =
                        Pose::new(Vec3::new(1.5, 0.8, -1.5), Some(Quat::from_angles(90.0, 0.0, 0.0)));
                    sk.send_event(StepperAction::add("StreamPlaybin", rtp_stream_uri_playbin_s));
                } else {
                    sk.send_event(StepperAction::Remove("StreamPlaybin".into()));
                }
            }
            Ui::same_line();
            if let Some(new_value) = Ui::toggle("RTSP server Playbin", &mut rtsp_server_playbin, None) {
                if new_value {
                    let uri_fmt = match coding { 
                        Coding::H264 => "rtsp://192.168.1.184:8554/test?media=video&clock-rate=90000&encoding-name=h264&payload=96&latency=0".to_string(),
                        Coding::H265 => "rtsp://192.168.1.184:8554/test?media=video&clock-rate=90000&encoding-name=h265&payload=96&latency=0".to_string(),
                        Coding::VP9 => "rtsp://192.168.1.184:8554/test?media=video&clock-rate=90000&encoding-name=vp9&payload=96&latency=0".to_string(),
                    };

                    let mut rtsp_server_playbin_s = Video1::new(VideoType::UriPlaybin { uri: uri_fmt });
                    // video_mkv_vp8.width = 854;
                    // video_mkv_vp8.height = 480;
                    rtsp_server_playbin_s.screen_pose =
                        Pose::new(Vec3::new(1.5, 0.8, -2.5), Some(Quat::from_angles(90.0, 0.0, 0.0)));
                    sk.send_event(StepperAction::add("ServerPlaybin", rtsp_server_playbin_s));
                } else {
                    sk.send_event(StepperAction::Remove("ServerPlaybin".into()));
                }
            }
            Ui::next_line();
            Ui::hseparator();
            if let Some(new_value) = Ui::toggle("Video MP4 H265", &mut video_h265_dec_active, None) {
                if new_value {
                    let uri_fmt = if let Some(dir_path) = get_external_path(&Some(sk.get_sk_info_clone())) {
                        let file_path = dir_path.join("videos").join("amaze4k.mp4");
                        if file_path.is_file() {
                            Log::diag(format!("File h265 : {:?}", file_path));
                            let path_str = file_path.to_str().unwrap().replace("\\", "/");
                            format!("file:///{}", path_str)
                        } else {
                            Log::warn(format!("No file h265 : {:?}", file_path));
                            "!!!!!!!No File".into()
                        }
                    } else {
                        Log::warn(format!("No external path{}", "!"));
                        "!!!!!!!No external path".into()
                    };
                    // launch video_h264
                    let mut video_h265 = Video1::new(VideoType::UriDecodebin { uri: uri_fmt });
                    video_h265.screen_pose =
                        Pose::new(Vec3::new(-0.5, 0.8, -1.5), Some(Quat::from_angles(90.0, 0.0, 0.0)));
                    sk.send_event(StepperAction::add("VideoH265_dec", video_h265));
                } else {
                    sk.send_event(StepperAction::Remove("VideoH265_dec".into()));
                }
            }
            Ui::same_line();
            if let Some(new_value) = Ui::toggle("Video MP4 H264", &mut video_h264_dec_active, None) {
                if new_value {
                    let uri_fmt = if let Some(dir_path) = get_external_path(&Some(sk.get_sk_info_clone())) {
                        let file_path = dir_path.join("videos").join("test.mp4");
                        if file_path.is_file() {
                            Log::diag(format!("File h264 : {:?}", file_path));
                            let path_str = file_path.to_str().unwrap().replace("\\", "/");
                            format!("file:///{}", path_str)
                        } else {
                            Log::warn(format!("No file h264 : {:?}", file_path));
                            "!!!!!!!No File".into()
                        }
                    } else {
                        Log::warn(format!("No external path{}", "!"));
                        "!!!!!!!No external path".into()
                    };
                    // launch video_h264
                    let mut video_h264 = Video1::new(VideoType::UriDecodebin { uri: uri_fmt });
                    video_h264.screen_pose =
                        Pose::new(Vec3::new(-0.5, 0.8, -1.5), Some(Quat::from_angles(90.0, 0.0, 0.0)));
                    sk.send_event(StepperAction::add("VideoH264_dec", video_h264));
                } else {
                    sk.send_event(StepperAction::Remove("VideoH264_dec".into()));
                }
            }
            Ui::same_line();            
            if let Some(new_value) = Ui::toggle("Video VP8 WEBM", &mut video_mkv_vp8_dec_active, None) {
                if new_value {
                    let uri_fmt = if let Some(dir_path) = get_external_path(&Some(sk.get_sk_info_clone())) {
                        let file_path = dir_path.join("videos").join("sintel_trailer-480p.webm");
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
                    let mut video_mkv_vp8 = Video1::new(VideoType::UriDecodebin { uri: uri_fmt });
                    // video_mkv_vp8.width = 854;
                    // video_mkv_vp8.height = 480;
                    video_mkv_vp8.screen_pose =
                        Pose::new(Vec3::new(1.5, 0.8, -1.5), Some(Quat::from_angles(90.0, 0.0, 0.0)));
                    sk.send_event(StepperAction::add("VideoVP8_dec", video_mkv_vp8));
                } else {
                    sk.send_event(StepperAction::Remove("VideoVP8_dec".into()));
                }
            }
            Ui::next_line();
            if let Some(new_value) = Ui::toggle("Video MP4 H264 playbin", &mut video_h264_play_active, None) {
                if new_value {
                    let uri_fmt = if let Some(dir_path) = get_external_path(&Some(sk.get_sk_info_clone())) {
                        let file_path = dir_path.join("videos").join("stereo_test.mp4");
                        if file_path.is_file() {
                            Log::diag(format!("File h264 : {:?}", file_path));
                            format!("file:///{}", file_path.to_str().unwrap())
                        } else {
                            Log::warn(format!("No file h264 : {:?}", file_path));
                            "!!!!!!!No File".into()
                        }
                    } else {
                        Log::warn(format!("No external path{}", "!"));
                        "!!!!!!No external path".into()
                    };
                    // launch video_h264
                    let mut video_h264 = Video1::new(VideoType::UriPlaybin { uri: uri_fmt });
                    video_h264.screen_pose =
                        Pose::new(Vec3::new(-0.5, -0.4, -1.5), Some(Quat::from_angles(90.0, 0.0, 0.0)));
                    sk.send_event(StepperAction::add("VideoH264_play", video_h264));
                } else {
                    sk.send_event(StepperAction::Remove("VideoH264_play".into()));
                }
            }
            Ui::same_line();
            if let Some(new_value) = Ui::toggle("Video VP8 MKV playbin", &mut video_vp8_play_active, None) {
                if new_value {
                    let uri_fmt = if let Some(dir_path) = get_external_path(&Some(sk.get_sk_info_clone())) {
                        let file_path = dir_path.join("videos").join("sintel_trailer-480p.mkv");
                        if file_path.is_file() {
                            Log::diag(format!("File VP8 : {:?}", file_path));
                            format!("file:///{}", file_path.to_str().unwrap())
                        } else {
                            Log::warn(format!("No file VP8 : {:?}", file_path));
                            "!!!!!!No File".into()
                        }
                    } else {
                        Log::warn(format!("No external path{}", "!"));
                        "!!!!!!No external path".into()
                    };
                    // launch video_h264
                    let mut video_vp8 = Video1::new(VideoType::UriPlaybin { uri: uri_fmt });
                    // video_vp8.width = 854;
                    // video_vp8.height = 480;
                    video_vp8.screen_pose =
                        Pose::new(Vec3::new(1.5, -0.4, -1.5), Some(Quat::from_angles(90.0, 0.0, 0.0)));
                    sk.send_event(StepperAction::add("VideoVP8_play", video_vp8));
                } else {
                    sk.send_event(StepperAction::Remove("VideoVP8_play".into()));
                }
            }
            Ui::same_line();
            if let Some(new_value) = Ui::toggle("Video VP8 WEBM HTTPS playbin", &mut video_vp8_https_play_active, None) {
                if new_value {
                    // launch video_vp8
                    let mut video_vp8 = Video1::new(VideoType::UriPlaybin {
                        uri: "https://gstreamer.freedesktop.org/data/media/sintel_trailer-480p.webm".into(),
                    });
                    video_vp8.screen_pose =
                        Pose::new(Vec3::new(3.5, -0.4, -1.5), Some(Quat::from_angles(90.0, 0.0, 0.0)));
                    sk.send_event(StepperAction::add("videoVP8HTTP_play", video_vp8));
                } else {
                    sk.send_event(StepperAction::Remove("videoVP8HTTP_play".into()));
                }
            }
            Ui::next_line();
            Ui::hseparator();
            if Ui::button("Exit", Some(Vec2::new(0.10, 0.10))) {
                sk.send_event(StepperAction::Quit("main".into(), "Main program call quit".into()));
                //sk.quit(None);
            }
            Ui::window_end();
        },
        |sk| Log::info(format!("QuitReason is {:?}", sk.get_quit_reason())),
    );
}
