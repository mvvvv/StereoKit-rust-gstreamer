use std::{
    cell::RefCell,
    f32::consts::PI,
    rc::Rc,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread::{self, sleep, JoinHandle},
    time::Duration,
};

use anyhow::bail;
use byte_slice_cast::AsSliceOf;
use gstreamer::{
    element_error,
    event::Eos,
    glib::{ffi::gpointer, object::Cast},
    prelude::{ElementExt, ElementExtManual, GstBinExt, GstBinExtManual, GstObjectExt, ObjectExt, PadExt},
    Bin, Bus, Element, ElementFactory, GhostPad, MessageRef, Pipeline, StreamType,
};

use gstreamer_app::{AppSink, AppSinkCallbacks};

use gstreamer_audio::{AudioBufferRef, AudioInfo, AudioLayout, AUDIO_FORMAT_F32};

use gstreamer_video::{VideoCapsBuilder, VideoFormat, VideoFrame, VideoInfo};
use stereokit_macros::IStepper;
#[cfg(target_os = "android")]
use stereokit_rust::tex::{TexFormat, TexType};
use stereokit_rust::{
    font::Font,
    framework::{IStepper, StepperId},
    material::Material,
    maths::{Bounds, Matrix, Pose, Quat, Vec2, Vec3},
    mesh::{Inds, Mesh, Vertex},
    prelude::*,
    sk::{MainThreadToken, SkInfo},
    sound::{Sound, SoundInst},
    sprite::Sprite,
    system::{Input, Log, Renderer, Text, TextStyle},
    tex::{Tex, TexSample},
    ui::{Ui, UiBtnLayout, UiMove, UiWin},
    util::{named_colors::RED, Time},
};

#[cfg(target_os = "android")]
use openxr_sys::SwapchainUsageFlags;
#[cfg(target_os = "android")]
use stereokit_rust::tools::xr_comp_layers::XrCompLayers;

#[derive(Debug, Clone, PartialEq)]
pub enum Coding {
    H264,
    H265,
    VP9,
}

#[derive(Debug)]
pub enum VideoType {
    None,
    RtpStream {
        port: i32,
        coding: Coding,
    },
    RtpStreamDecodebin {
        port: i32,
        coding: Coding,
    },
    #[cfg(target_os = "android")]
    RtpStreamAndroid {
        port: i32,
        coding: Coding,
    },
    UriDecodebin {
        uri: String,
    },
    UriPlaybin {
        uri: String,
    },
}

pub struct VideoRepo {
    id_btn_show_hide_param: String,
    id_window_param: String,
    show_param: bool,
    sprite_hide_param: Sprite,
    sprite_show_param: Sprite,
    id_handle: String,
    id_texture: String,
    id_left_sound: String,
    id_right_sound: String,
    id_slider_distance: String,
    id_slider_size: String,
    id_slider_flattening: String,
}

impl VideoRepo {
    pub fn new(id: String) -> Self {
        Self {
            show_param: false,
            sprite_hide_param: Sprite::close(),
            sprite_show_param: Sprite::from_file("icons/hamburger.png", None, None).unwrap_or_default(),
            id_btn_show_hide_param: id.clone() + "_btn_show_hide",
            id_window_param: id.clone() + "_window_param",
            id_handle: id.clone() + "_handle",
            id_texture: id.clone() + "_texture",
            id_left_sound: id.clone() + "_left_sound",
            id_right_sound: id.clone() + "_right_sound",
            id_slider_distance: id.clone() + "_slider_distance",
            id_slider_size: id.clone() + "_slider_size",
            id_slider_flattening: id.clone() + "_slider_radius",
        }
    }
}

/// The video stepper
#[derive(IStepper)]
pub struct Video1 {
    id: StepperId,
    sk_info: Option<Rc<RefCell<SkInfo>>>,
    shutdown_completed: bool,

    repo: VideoRepo,
    video_type: VideoType,
    video_info: Option<VideoInfo>,
    audio_info: Option<AudioInfo>,
    pub width: i32,
    pub height: i32,
    pub screen_distance: f32,
    pub screen_flattening: f32,
    pub screen_size: Vec2,
    pub screen_diagonal: f32,
    pub screen_pose: Pose,
    pub screen: Mesh,
    pub sound_spacing_factor: f32,
    pub text: String,
    pub transform: Matrix,
    pub text_style: Option<TextStyle>,
    video_material: Material,
    pipeline: Option<Pipeline>,
    bus: Option<Bus>,
    bus_thread: Option<JoinHandle<bool>>,
    first_step: bool,
    stream_running: Arc<AtomicBool>,
    sound_left: Sound,
    sound_left_inst: Option<SoundInst>,
    sound_right: Sound,
    sound_right_inst: Option<SoundInst>,
    #[cfg(target_os = "android")]
    xr_comp_layers: Option<XrCompLayers>,
    #[cfg(target_os = "android")]
    android_swapchain: Option<openxr_sys::Swapchain>,
}

unsafe impl Send for Video1 {}

/// This code may be called in some threads, so no StereoKit code
impl Default for Video1 {
    fn default() -> Self {
        let screen_size = Vec2::new(3.840, 2.160);
        let screen_diagonal = (screen_size.x.powf(2.0) + screen_size.y.powf(2.0)).sqrt();
        let video_material = Material::unlit().copy();

        Self {
            id: "Video1".to_string(),
            sk_info: None,
            shutdown_completed: false,

            repo: VideoRepo::new("Video1".to_string()),
            video_type: VideoType::None,
            width: 3840,
            height: 2160,
            video_info: None,
            audio_info: None,
            screen_distance: 2.20,
            screen_flattening: 0.99,
            screen_size,
            screen_diagonal,
            screen_pose: Pose::IDENTITY,
            screen: Mesh::new(),
            sound_spacing_factor: 3.0,
            text: "Video1".to_owned(),
            transform: Matrix::t_r(
                Vec3::new(0.0, 2.0, -2.5), //
                Quat::from_angles(0.0, 180.0, 0.0),
            ),
            text_style: Some(Text::make_style(Font::default(), 0.3, RED)),
            video_material,
            pipeline: None,
            bus: None,
            bus_thread: None,
            first_step: true,
            stream_running: Arc::new(AtomicBool::new(false)),
            sound_left: Sound::click(),
            sound_left_inst: None,
            sound_right: Sound::click(),
            sound_right_inst: None,
            #[cfg(target_os = "android")]
            xr_comp_layers: None,
            #[cfg(target_os = "android")]
            android_swapchain: None,
        }
    }
}

/// All the code here run in the main thread
impl Video1 {
    /// Create the video player
    pub fn new(video_type: VideoType) -> Self {
        Self { video_type, ..Default::default() }
    }

    fn start(&mut self) -> bool {
        self.repo = VideoRepo::new(self.id.clone());

        self.sound_left = Sound::create_stream(200.0).unwrap_or_default();
        self.sound_left.id(&self.repo.id_left_sound);
        self.sound_right = Sound::create_stream(200.0).unwrap_or_default();
        self.sound_right.id(&self.repo.id_right_sound);

        self.screen_pose = Input::get_head() * Matrix::r(Quat::from_angles(0.0, 180.0, 0.0));
        self.adapt_screen();

        self.video_info = VideoInfo::builder(VideoFormat::Rgba, self.width as u32, self.height as u32).build().ok();
        self.audio_info =
            AudioInfo::builder(AUDIO_FORMAT_F32, 48000, 2).layout(AudioLayout::NonInterleaved).build().ok();

        if let Err(error) = match &self.video_type {
            VideoType::RtpStream { port, coding } => self.init_rtp_stream(*port, coding.clone()),
            VideoType::RtpStreamDecodebin { port, coding } => self.init_rtp_stream_decodebin(*port, coding.clone()),
            #[cfg(target_os = "android")]
            VideoType::RtpStreamAndroid { port, coding } => self.init_rtp_stream_android(*port, coding.clone()),
            VideoType::UriDecodebin { uri } => self.init_uri_decodebin(uri.clone()),
            VideoType::UriPlaybin { uri } => self.init_uri_playbin(uri.clone()),
            otherwise => {
                Log::err(format!("Unable to launch video type : {:?}", otherwise));
                return false;
            }
        } {
            Log::err(format!("Unable to initialize video : {:?}", error));
            false
        } else {
            true
        }
    }

    /// Called from IStepper::step, here you can check the event report
    fn check_event(&mut self, _id: &StepperId, _key: &str, _value: &str) {
        // if key == "CStepper" {
        //     self.enabled = value.parse().unwrap_or(false)
        // }
    }

    /// Called from IStepper::step, after check_event here you can draw your UI and scene
    pub fn draw(&mut self, token: &MainThreadToken) {
        if let Some(pipeline) = &self.pipeline {
            if let Some(bus) = &self.bus {
                Log::diag(format!("{:?}", bus.pop()));
            }
            if self.first_step {
                self.first_step = false;
                self.stream_running.store(true, Ordering::SeqCst);
                let _res = pipeline.set_state(gstreamer::State::Playing);
                self.sound_left_inst = Some(self.sound_left.play(self.sound_position(-1), Some(1.0)));

                self.sound_right_inst = Some(self.sound_right.play(self.sound_position(1), Some(1.0)));
            } else if !self.stream_running.load(Ordering::Relaxed) {
                self.close_pipeline();
            }
        }

        let screen_transform = self.screen_param();

        Renderer::add_mesh(token, &self.screen, &self.video_material, screen_transform, None, None);

        Text::add_at(token, &self.text, self.transform, self.text_style, None, None, None, None, None, None);
    }

    /// Here is managed the screen position, its rotundity, size and distance
    fn screen_param(&mut self) -> Matrix {
        const GRAB_X_MARGIN: f32 = 0.4;

        const MAX_DISTANCE: f32 = 6.0;

        const MAX_DIAGONAL: f32 = 15.0;
        const MIN_DIAGONAL: f32 = 0.2;
        let bounds = self.screen.get_bounds();

        let factor_size = (self.screen_distance.max(1.0).powf(2.0) + self.screen_diagonal.max(1.0).powf(2.0)).sqrt();

        let grab_position = Vec3::new(
            0.0, //
            self.screen_size.y / 2.0 + 0.05 * factor_size,
            bounds.center.z,
        );
        let grab_dimension = Vec3::new(
            factor_size * 0.2, //
            factor_size * 0.01,
            factor_size * 0.01,
        );
        if Ui::handle(
            &self.repo.id_handle,
            &mut self.screen_pose,
            Bounds::new(grab_position, grab_dimension),
            true,
            Some(UiMove::Exact),
            None,
        ) {
            let head = Input::get_head();
            self.screen_pose.position = head.position;
        }
        let screen_transform = self.screen_pose.to_matrix(None);

        let mut adapt = false;
        if self.repo.show_param {
            let info_position = Vec3::new(bounds.center.x, bounds.center.y, GRAB_X_MARGIN * 1.5);
            let mut window_pose = Pose::new(info_position, None) * screen_transform;
            Ui::window_begin(
                &self.repo.id_window_param,
                &mut window_pose,
                Some(Vec2::new(0.4, 0.2)),
                Some(UiWin::Body),
                Some(UiMove::None),
            );

            if Ui::button_img(
                &self.repo.id_btn_show_hide_param,
                &self.repo.sprite_hide_param,
                Some(UiBtnLayout::CenterNoText),
                None,
                None,
            ) {
                self.repo.show_param = false;
            }
            Ui::label("Distance", None, true);
            Ui::same_line();
            Ui::label(format!("{:.2}", self.screen_distance), None, true);
            Ui::same_line();
            let old_value = self.screen_distance;
            if let Some(_new_value) = Ui::hslider(
                &self.repo.id_slider_distance,
                &mut self.screen_distance,
                GRAB_X_MARGIN * 2.0,
                MAX_DISTANCE,
                None,
                None,
                None,
                None,
            ) {
                let max_size = self.screen_distance * PI;
                let screen_size = self.screen_size;
                if screen_size.x > max_size || self.screen_size.y > max_size {
                    self.screen_distance = old_value;
                } else {
                    adapt = true;
                }
            }

            Ui::label("Diagonal", None, true);
            Ui::same_line();
            Ui::label(format!("{:.2}", self.screen_diagonal), None, true);
            Ui::same_line();
            let old_value = self.screen_diagonal;
            if let Some(new_value) = Ui::hslider(
                &self.repo.id_slider_size,
                &mut self.screen_diagonal,
                MIN_DIAGONAL,
                MAX_DIAGONAL,
                None,
                None,
                None,
                None,
            ) {
                let max_size = self.screen_distance * PI;
                let screen_size = self.screen_size * new_value / old_value;
                if screen_size.x > max_size || self.screen_size.y > max_size {
                    self.screen_diagonal = old_value;
                } else {
                    self.screen_size = screen_size;
                    adapt = true;
                }
            }

            Ui::label("Curvature", None, true);
            Ui::same_line();
            Ui::label(format!("{:.2}", self.screen_flattening), None, true);
            Ui::same_line();
            if let Some(new_value) = Ui::hslider(
                &self.repo.id_slider_flattening,
                &mut self.screen_flattening,
                0.0,
                1.0,
                None,
                None,
                None,
                None,
            ) {
                self.screen_flattening = new_value;
                adapt = true;
            }
        } else {
            let info_position = Vec3::new(
                0.0, //
                self.screen_size.y / 2.0 + 0.04 * factor_size,
                bounds.center.z,
            );
            let mut window_pose = Pose::new(info_position, None) * screen_transform;
            Ui::window_begin(&self.repo.id_window_param, &mut window_pose, None, Some(UiWin::Body), Some(UiMove::None));
            if Ui::button_img(
                &self.repo.id_btn_show_hide_param,
                &self.repo.sprite_show_param,
                Some(UiBtnLayout::CenterNoText),
                Some(Vec2::new(0.03 * factor_size, 0.03 * factor_size)),
                None,
            ) {
                self.repo.show_param = true;
                let head = Input::get_head();
                self.screen_pose.position = head.position;
            }
        }
        Ui::window_end();
        if adapt {
            self.adapt_screen();
        }
        screen_transform
    }

    /// Calculate sound position. If factor < 0 this is for left else for right
    fn sound_position(&self, factor: i8) -> Vec3 {
        let up = self.screen_pose.get_up();
        let forward = self.screen_pose.get_forward();
        let cross = Vec3::cross(up, forward);
        cross * factor as f32 * self.sound_spacing_factor
    }

    fn adapt_screen(&mut self) {
        let distance = self.screen_distance;
        let flattening = if self.screen_flattening <= 0.0 { 500.0 } else { 1.0 / self.screen_flattening - 1.0 };
        let radius = distance + flattening;

        let width = self.screen_size.x;
        let height = self.screen_size.y;

        self.screen = {
            let mut verts: Vec<Vertex> = vec![];
            let mut inds: Vec<Inds> = vec![];

            let aspect_ratio = width / height;

            let perimeter = 2.0 * PI * radius;

            let subdiv_v = 30u32;
            let subdiv_u = (subdiv_v as f32 * aspect_ratio) as u32;

            let angle_v = 2.0 * PI * height / perimeter;
            let angle_u = 2.0 * PI * width / perimeter;
            let delta_v = angle_v / subdiv_v as f32;
            let delta_u = angle_u / subdiv_u as f32;

            for j in 0..subdiv_v {
                let v = -angle_v / 2.0 + (j as f32 * delta_v) + PI / 2.0;
                for i in 0..subdiv_u {
                    let u = -angle_u / 2.0 + (i as f32 * delta_u) + PI / 2.0;
                    let x = radius * v.sin() * u.cos();
                    let y = radius * v.cos();
                    let z = radius * v.sin() * u.sin() - flattening;

                    verts.push(Vertex::new(
                        Vec3::new(x, y, z), //
                        Vec3::FORWARD,
                        Some(Vec2::new(i as f32 / (subdiv_u - 1) as f32, j as f32 / (subdiv_v - 1) as f32)),
                        None,
                    ));

                    //Log::diag(format!("vertex: {} {} {}", x, y, z));

                    let nb_row = subdiv_u;
                    let last_line = j == subdiv_v - 1;
                    if !last_line {
                        let row_is_even = i % 2 == 0;
                        let last_row = i == nb_row - 1;
                        let a = j * nb_row + i;
                        let b = j * nb_row + i + 1;
                        let c = (j + 1) * nb_row + i;
                        if row_is_even {
                            if !last_row {
                                inds.push(a);
                                inds.push(b);
                                inds.push(c);
                                inds.push(a);
                                inds.push(c);
                                inds.push(b);
                                //Log::diag(format!("inds: a{} b{} c{}", a, b, c));
                            }
                        } else {
                            let c_previous = (j + 1) * nb_row + i - 1;
                            let c_following = (j + 1) * nb_row + i + 1;
                            inds.push(a);
                            inds.push(c);
                            inds.push(c_previous);
                            inds.push(a);
                            inds.push(c_previous);
                            inds.push(c);
                            if !last_row {
                                inds.push(a);
                                inds.push(c_following);
                                inds.push(c);
                                inds.push(a);
                                inds.push(c);
                                inds.push(c_following);

                                inds.push(a);
                                inds.push(b);
                                inds.push(c_following);
                                inds.push(a);
                                inds.push(c_following);
                                inds.push(b);
                                //Log::diag(format!("inds: a{} b{} c{} c-{} c+{} ", a, b, c, c_previous, c_following));
                            } else {
                                //Log::diag(format!("inds: a{} c{} c-{} ", a, c, c_previous));
                            }
                        }
                    }
                }
            }

            let mut mesh = Mesh::new();
            mesh.set_inds(inds.as_slice());
            mesh.set_verts(verts.as_slice(), true);

            mesh
        };
    }

    /// Close the pipeline
    ///
    pub fn close_pipeline(&mut self) {
        if let Some(pipeline) = &self.pipeline.clone() {
            Log::diag(format!("Closing Video1/{} !!!", self.id));
            if let Some(sound_inst) = self.sound_left_inst.take() {
                sound_inst.stop()
            };
            if let Some(sound_inst) = self.sound_right_inst.take() {
                sound_inst.stop()
            };
            for sink in [self.id.clone() + "_sink_video", self.id.clone() + "_sink_audio"] {
                if let Some(element) = pipeline.by_name(&sink) {
                    Log::diag(format!("Disposing {} callbacks", sink));
                    if let Ok(sink) = element.downcast::<AppSink>() {
                        sink.set_callbacks(AppSinkCallbacks::builder().build());
                    }
                }
            }
            if let Some(join_h) = &self.bus_thread {
                if !join_h.is_finished() {
                    //pipeline.set_message_forward(true);
                    if !pipeline.send_event(Eos::new()) {
                        Log::warn(format!("Unable to send EOS to {}", self.id));
                    }
                    self.stream_running.store(false, Ordering::SeqCst);
                } else {
                    self.bus_thread = None
                }
            }
        }
        self.bus = None;
        self.pipeline = None;
    }

    /// init a video rtp stream for decodebin
    ///
    fn init_rtp_stream(&mut self, port: i32, coding: Coding) -> Result<(), anyhow::Error> {
        let (tex_id, pipeline) = self.init_player()?;

        let (up_code, low_code) = match coding {
            Coding::H264 => ("H264", "h264"),
            Coding::H265 => ("H265", "h265"),
            Coding::VP9 => ("VP9", "vp9"),
        };

        let rtp_caps = gstreamer::Caps::builder("application/x-rtp")
            .field("encoding-name", up_code)
            .field("payload", "96")
            .build();

        let udpsrc = gstreamer::ElementFactory::make("udpsrc")
            .property("port", port)
            .property("caps", &rtp_caps)
            .property("buffer-size", 8388608)
            .build()?;
        let queue1 = ElementFactory::make("queue")
            .property("max-size-buffers", 0u32) // Unlimited buffers to avoid dropping RTP packets
            .build()?;
        let rtpjitterbuffer = ElementFactory::make("rtpjitterbuffer")
            .property("latency", 0u32)
            .property("do-lost", true)
            .build()?;
        let rtp_depay = ElementFactory::make(&format!("rtp{}depay", low_code)).build()?;
        let parse = ElementFactory::make(&format!("{}parse", low_code)).build()?;
        let videoconvert = ElementFactory::make("autovideoconvert").build()?;
        //let videoscale = ElementFactory::make("videoscale").build()?;
        let queue2 = ElementFactory::make("queue")
            .property_from_str("leaky", "downstream")
            .property("max-size-buffers", 1u32)
            .build()?;
        let video_info = match &self.video_info {
            Some(info) => info,
            None => bail!("No video info for {}", self.id),
        };

        #[allow(unused_assignments)]
        let mut appsink_caps = VideoCapsBuilder::new().build();
        if cfg!(target_os = "android") {
            #[cfg(feature = "gl")]
            {
                appsink_caps = VideoCapsBuilder::new()
                    .features([gstreamer_gl::CAPS_FEATURE_MEMORY_GL_MEMORY])
                    .field("texture-target", "2D")
                    //.framerate((60, 1).into())
                    .format(video_info.format())
                    .width(self.width)
                    .height(self.height)
                    .build()
            }
        } else {
            appsink_caps = VideoCapsBuilder::new()
                .format(video_info.format())
                .width(video_info.width() as i32)
                .height(video_info.height() as i32)
                .build() //  video_info.to_caps()?;
        };
        let appsink = AppSink::builder()
            .name(self.id.clone() + "_sink_video")
            .caps(&appsink_caps)
            .sync(false)
            .max_buffers(1)
            .drop(true)
            .build();

        if cfg!(target_os = "android") {
            let decoder_name = match coding {
                Coding::H264 => "amcviddec-omxqcomvideodecoderavc",
                Coding::H265 => "amcviddec-omxqcomvideodecoderhevc",
                Coding::VP9 => "amcviddec-omxqcomvideodecodervp9",
            };
            let decode = ElementFactory::make(decoder_name).build()?;
            let glcolorconvert = ElementFactory::make("glcolorconvert").build()?;
            let elements = vec![
                &udpsrc,
                &queue1,
                &rtpjitterbuffer,
                &rtp_depay,
                &parse,
                &decode,
                &queue2,
                &glcolorconvert, //
                &videoconvert,
                //&videoscale,
                appsink.upcast_ref(),
            ];

            add_and_link(elements, pipeline.as_ref())?;
        } else {
            let (nv_dec, av_dec) = match coding {
                Coding::H264 => ("nvh264dec", "avdec_h264"),
                Coding::H265 => ("nvh265dec", "avdec_h265"),
                Coding::VP9 => ("nvvp9dec", "avdec_vp9"),
            };
            let decode = ElementFactory::make(nv_dec).build().unwrap_or(ElementFactory::make(av_dec).build()?);
            let elements = vec![
                &udpsrc,
                &queue1,
                //&rtpjitterbuffer,
                &rtp_depay,
                &parse,
                &decode,
                &videoconvert,
                //&videoscale,
                appsink.upcast_ref(),
            ];

            add_and_link(elements, pipeline.as_ref())?;
        }

        let video_tex = Tex::find(tex_id)?;
        Video1::set_video_callback(&appsink, video_tex, video_info.clone())?;
        if self.bus_report(&pipeline) {
            self.pipeline = Some(pipeline);
            Ok(())
        } else {
            bail!("Unable to launch_and_watch for {}", self.id)
        }
    }

    /// init a video rtp stream for decodebin or decodebin3
    ///
    fn init_rtp_stream_decodebin(&mut self, port: i32, coding: Coding) -> Result<(), anyhow::Error> {
        let (tex_id, pipeline) = self.init_player()?;

        let (up_code, low_code) = match coding {
            Coding::H264 => ("H264", "h264"),
            Coding::H265 => ("H265", "h265"),
            Coding::VP9 => ("VP9", "vp9"),
        };
        let rtp_caps = gstreamer::Caps::builder("application/x-rtp")
            //.field("format", "BGRA")
            .field("encoding-name", up_code)
            .field("payload", "96")
            .build();

        let udpsrc = gstreamer::ElementFactory::make("udpsrc")
            .property("port", port)
            .property("caps", &rtp_caps)
            .property("buffer-size", 8388608)
            .build()?;
        let queue1 = ElementFactory::make("queue").property("max-size-buffers", 0u32).build()?;
        let rtpjitterbuffer = ElementFactory::make("rtpjitterbuffer")
            .property("latency", 0u32)
            .property("do-lost", true)
            .build()?;
        let rtp_depay = ElementFactory::make(&format!("rtp{}depay", low_code)).build()?;
        let parse = ElementFactory::make(&format!("{}parse", low_code)).build()?;
        let queue2 = ElementFactory::make("queue")
            .property_from_str("leaky", "downstream")
            .property("max-size-buffers", 1u32)
            .build()?;
        let decode = ElementFactory::make("decodebin3").build()?;
        let elements = vec![
            &udpsrc,
            &queue1,
            &rtpjitterbuffer, //
            &rtp_depay,
            &parse,
            &queue2,
            &decode,
        ];

        add_and_link(elements, pipeline.as_ref())?;

        let pipeline_weak = pipeline.downgrade();

        self.connect_pad(decode, pipeline_weak, tex_id, true)?;

        if self.bus_report(&pipeline) {
            self.pipeline = Some(pipeline);
            Ok(())
        } else {
            bail!("Unable to launch_and_watch for {}", self.id)
        }
    }

    /// init a video rtp stream optimized for Android using XR_KHR_android_surface_swapchain
    /// This uses the Android hardware decoder (amcviddec) and renders directly to an Android Surface
    /// which is then submitted as an OpenXR composition layer for optimal performance.
    #[cfg(target_os = "android")]
    fn init_rtp_stream_android(&mut self, port: i32, coding: Coding) -> Result<(), anyhow::Error> {
        let pipeline = Pipeline::default();

        let (up_code, low_code, decoder_name) = match coding {
            Coding::H264 => ("H264", "h264", "amcviddec-omxqcomvideodecoderavc"),
            Coding::H265 => ("H265", "h265", "amcviddec-omxqcomvideodecoderhevc"),
            Coding::VP9 => ("VP9", "vp9", "amcviddec-omxqcomvideodecodervp9"),
        };

        // Create XrCompLayers for Android surface swapchain
        let xr_comp_layers =
            XrCompLayers::new().ok_or_else(|| anyhow::anyhow!("Failed to create XrCompLayers for Android surface"))?;

        // Create Android surface swapchain
        let (swapchain_handle, android_surface) = xr_comp_layers
            .try_make_android_swapchain(
                self.width as u32,
                self.height as u32,
                SwapchainUsageFlags::COLOR_ATTACHMENT | SwapchainUsageFlags::SAMPLED,
                false,
            )
            .ok_or_else(|| anyhow::anyhow!("Failed to create Android surface swapchain"))?;

        Log::diag(format!(
            "Created Android surface swapchain: handle={:?}, surface={:?}, size={}x{}",
            swapchain_handle, android_surface, self.width, self.height
        ));

        // Convert Java Surface to ANativeWindow
        let surface = android_surface as jni::sys::jobject;
        let native_window = {
            let ctx = ndk_context::android_context();
            let vm = unsafe { jni::JavaVM::from_raw(ctx.vm() as _) }?;
            let env = vm.attach_current_thread()?;

            unsafe { ndk_sys::ANativeWindow_fromSurface(env.get_native_interface(), surface) }
        };

        if native_window.is_null() {
            anyhow::bail!("Could not get ANativeWindow from Surface");
        }
        Log::diag(format!("Got ANativeWindow: {:?}", native_window));

        // Build the GStreamer pipeline
        let rtp_caps = gstreamer::Caps::builder("application/x-rtp")
            .field("encoding-name", up_code)
            .field("payload", "96")
            .build();

        let udpsrc = gstreamer::ElementFactory::make("udpsrc")
            .property("port", port)
            .property("caps", &rtp_caps)
            .property("buffer-size", 8388608)
            .build()?;

        let queue1 = ElementFactory::make("queue")
            .property("max-size-buffers", 500u32)
            .property("max-size-bytes", 0u32)
            .property("max-size-time", 0u64)
            .build()?;

        let rtp_depay = ElementFactory::make(&format!("rtp{}depay", low_code)).build()?;
        let parse = ElementFactory::make(&format!("{}parse", low_code)).build()?;

        let queue2 = ElementFactory::make("queue")
            .property("max-size-buffers", 500u32)
            .property("max-size-bytes", 0u32)
            .property("max-size-time", 0u64)
            .build()?;

        // Use Android hardware decoder
        let decoder = ElementFactory::make(decoder_name).build().or_else(|_| {
            Log::warn(format!("Fallback to decodebin3 as {} is not available", decoder_name));
            ElementFactory::make("decodebin3").build()
        })?;

        // Use glimagesink to render to the Android Surface
        let sink = ElementFactory::make("glimagesink")
            .property("sync", false)
            .property("force-aspect-ratio", false)
            .build()
            .or_else(|_| {
                Log::warn("glimagesink not available, trying fakesink");
                ElementFactory::make("fakesink").property("sync", false).build()
            })?;

        Log::diag(format!("Using sink element: {}", sink.name()));

        // Try VideoOverlay interface
        use gstreamer_video::prelude::VideoOverlayExtManual;
        if let Ok(overlay) = sink.clone().dynamic_cast::<gstreamer_video::VideoOverlay>() {
            unsafe {
                overlay.set_window_handle(native_window as usize);
            }
            Log::diag("Set window handle via VideoOverlay");
        } else {
            Log::warn("Sink does not implement VideoOverlay, video may not display");
        }

        let glupload = ElementFactory::make("glupload").build()?;
        let glcolorconvert = ElementFactory::make("glcolorconvert").build()?;

        let elements = vec![&udpsrc, &queue1, &rtp_depay, &parse, &queue2, &decoder, &glupload, &glcolorconvert, &sink];

        add_and_link(elements, pipeline.as_ref())?;

        // Create StereoKit texture and associate it with the Android native surface
        let tex_id = self.repo.id_texture.clone();
        // Must create an empty render_target, set_native_surface can't replace loaded image data
        let mut video_tex =
            Tex::render_target(self.width as usize, self.height as usize, None, None, None).unwrap_or_default();
        video_tex.id(&tex_id).sample_mode(TexSample::Point);

        // Associate the native window with the StereoKit texture BEFORE setting it on the material
        // This allows OpenXR to composite the surface directly without extra copies
        Log::diag("set_native_surface >>");
        Log::diag(format!("abandonned native_surface={:?}", video_tex.get_native_surface()));
        unsafe {
            video_tex.set_native_surface(
                android_surface as *mut core::ffi::c_void,
                TexType::Image,
                XrCompLayers::to_native_format(TexFormat::RGBA32),
                self.width,
                self.height,
                1,
                false,
            );
        }
        Log::diag(format!("new native_surface={:?}", video_tex.get_native_surface()));
        Log::diag("<< set_native_surface");

        let material_id = self.id.clone() + "material_video";
        self.video_material.id(&material_id).diffuse_tex(&video_tex);

        Log::diag(format!(
            "Android RTP stream initialized on port {} with {} codec, swapchain={:?}",
            port, up_code, swapchain_handle
        ));

        if self.bus_report(&pipeline) {
            self.pipeline = Some(pipeline);
            self.xr_comp_layers = Some(xr_comp_layers);
            self.android_swapchain = Some(swapchain_handle);
            Ok(())
        } else {
            xr_comp_layers.destroy_android_swapchain(swapchain_handle);
            bail!("Unable to launch_and_watch for {}", self.id)
        }
    }

    /// gl experiment
    fn init_uri_playbin(&mut self, uri: String) -> Result<(), anyhow::Error> {
        let (tex_id, _) = self.init_player()?;
        let playbin = ElementFactory::make("playbin3").property("uri", &uri).build()?;

        //-----------------------------------
        //--- audio
        let equalizer = ElementFactory::make("equalizer-3bands").build()?;
        let a_convert = ElementFactory::make("audioconvert").build()?;
        let resample = ElementFactory::make("audioresample").build()?;
        let mut elements = vec![&equalizer, &a_convert, &resample];
        let audio_info = match &self.audio_info {
            Some(info) => info.clone(),
            None => bail!("No audio info for {}", self.id),
        };
        let audio_appsink =
            AppSink::builder().name(self.id.clone() + "_sink_audio").caps(&audio_info.to_caps()?).build();

        let audio_bin = Bin::with_name("audio_sink_bin");

        elements.push(audio_appsink.upcast_ref());
        add_and_link(elements, &audio_bin)?;

        if let Some(pad) = equalizer.static_pad("sink") {
            let ghost_pad = GhostPad::with_target(&pad)?;
            ghost_pad.set_active(true)?;
            audio_bin.add_pad(&ghost_pad)?;
        }

        equalizer.set_property("band1", -0.0);
        equalizer.set_property("band2", -0.0);

        let sound_left = Sound::find(&self.repo.id_left_sound)?;
        let sound_right = Sound::find(&self.repo.id_right_sound)?;
        Video1::set_audio_callback(audio_appsink, sound_left, sound_right, audio_info)?;

        playbin.set_property("audio-sink", audio_bin);

        //-----------------------------------
        //--- video
        let queue1 = ElementFactory::make("queue").build()?;
        let v_convert = ElementFactory::make("autovideoconvert").build()?;
        let mut elements = vec![&queue1, &v_convert];

        let video_info = match &self.video_info {
            Some(info) => info.clone(),
            None => bail!("No video info for {}", self.id),
        };

        #[allow(unused_assignments)]
        let mut appsink_caps = VideoCapsBuilder::new().build();
        if cfg!(target_os = "android") {
            #[cfg(feature = "gl")]
            {
                appsink_caps = VideoCapsBuilder::new()
                    .features([gstreamer_gl::CAPS_FEATURE_MEMORY_GL_MEMORY])
                    .field("texture-target", "2D")
                    .framerate((60, 1).into())
                    .format(VideoFormat::Rgba)
                    .width(video_info.width() as i32)
                    .height(video_info.height() as i32)
                    .build()
            }
        } else {
            appsink_caps = VideoCapsBuilder::new()
                .format(video_info.format())
                .width(video_info.width() as i32)
                .height(video_info.height() as i32)
                .build() //  video_info.to_caps()?;
        };
        let video_appsink = AppSink::builder()
            .name(self.id.clone() + "_sink_video")
            .drop(true)
            .max_buffers(1)
            .sync(true)
            .caps(&appsink_caps)
            .build();

        let video_bin = Bin::with_name("video_sink_bin");
        if cfg!(target_os = "android") {
            // let glupload = ElementFactory::make("glupload").build()?;
            // elements.push(&glupload);
            let glcolorconvert = ElementFactory::make("glcolorconvert").build()?;
            elements.push(&glcolorconvert);
            elements.push(video_appsink.upcast_ref());
            add_and_link(elements, &video_bin)?;
        } else {
            elements.push(video_appsink.upcast_ref());
            add_and_link(elements, &video_bin)?;
        }

        if let Some(pad) = queue1.static_pad("sink") {
            let ghost_pad = GhostPad::with_target(&pad)?;
            ghost_pad.set_active(true)?;
            video_bin.add_pad(&ghost_pad)?;
        }

        let video_tex = Tex::find(tex_id)?;
        Video1::set_video_callback(&video_appsink, video_tex, video_info)?;
        playbin.set_property("video-sink", video_bin);

        //--- A bus and we can run
        if let Ok(pipeline) = playbin.downcast::<Pipeline>() {
            if self.bus_report(&pipeline) {
                self.pipeline = Some(pipeline);
                Ok(())
            } else {
                bail!("Unable to launch_and_watch for {}", self.id)
            }
        } else {
            bail!("Unable to get pipeline downcast for {}", self.id)
        }
    }

    /// init a video
    ///
    fn init_uri_decodebin(&mut self, uri: String) -> Result<(), anyhow::Error> {
        let (tex_id, pipeline) = self.init_player()?;

        let decode = if uri.starts_with("file:") || uri.starts_with("https://") {
            let uridecodebin = ElementFactory::make("uridecodebin3").property("uri", uri).build()?;

            pipeline.add_many([&uridecodebin])?;
            uridecodebin
        } else {
            let src = ElementFactory::make("filesrc").property("location", uri).build()?;
            let decodebin = ElementFactory::make("decodebin3").build()?;

            pipeline.add_many([&src, &decodebin])?;
            Element::link_many([&src, &decodebin])?;
            decodebin
        };

        let pipeline_weak = pipeline.downgrade();

        self.connect_pad(decode, pipeline_weak, tex_id, false)?;

        if self.bus_report(&pipeline) {
            self.pipeline = Some(pipeline);
            Ok(())
        } else {
            bail!("Unable to launch_and_watch for {}", self.id)
        }
    }

    /// Decodebin & Decodebin3 have to know what is the stream in order to create the right decode chain.
    fn connect_pad(
        &mut self,
        decode: Element,
        pipeline_weak: gstreamer::glib::WeakRef<Pipeline>,
        tex_id: String,
        low_latency: bool,
    ) -> Result<(), anyhow::Error> {
        let sound_left_id = self.repo.id_left_sound.clone();
        let sound_right_id = self.repo.id_right_sound.clone();
        let id = self.id.clone();
        let video_info = match &self.video_info {
            Some(info) => info.clone(),
            None => bail!("No video info for {}", self.id),
        };
        let audio_info = match &self.audio_info {
            Some(info) => info.clone(),
            None => bail!("No audio info for {}", self.id),
        };

        decode.connect_pad_added(move |_dbin, src_pad| {
            Log::diag("connect");
            let Some(pipeline) = pipeline_weak.upgrade() else {
                return;
            };
            let (is_audio, is_video) = if let Some(stream) = src_pad.stream() {
                match stream.stream_type() {
                    StreamType::VIDEO => (false, true),
                    StreamType::AUDIO => (true, false),
                    _ => (false, false),
                }
            } else {
                (false, false)
            };

            let id = id.clone();
            let sound_left_id = sound_left_id.clone();
            let sound_right_id = sound_right_id.clone();

            let tex_id = tex_id.clone();
            let video_info = video_info.clone();
            let audio_info = audio_info.clone();

            let insert_sink = move |is_audio, is_video| -> Result<(), anyhow::Error> {
                if is_audio {
                    let queue = ElementFactory::make("queue").build()?;
                    let convert = ElementFactory::make("audioconvert").build()?;
                    let resample = ElementFactory::make("audioresample").build()?;
                    let appsink = AppSink::builder()
                        .name(id.clone() + "_sink_audio")
                        .drop(true)
                        .caps(&audio_info.to_caps()?)
                        .build();

                    let elements = vec![&queue, &convert, &resample, appsink.upcast_ref()];
                    add_and_link(elements, pipeline.as_ref())?;

                    let sink_pad = queue.static_pad("sink").expect("queue has no sinkpad");
                    src_pad.link(&sink_pad)?;

                    let sound_left = Sound::find(&sound_left_id)?;
                    let sound_right = Sound::find(&sound_right_id)?;
                    Video1::set_audio_callback(appsink, sound_left, sound_right, audio_info)?;
                } else if is_video {
                    let queue = if low_latency {
                        ElementFactory::make("queue")
                            .property_from_str("leaky", "downstream")
                            .property("max-size-buffers", 1u32)
                            .build()?
                    } else {
                        ElementFactory::make("queue").build()?
                    };
                    // Avoid autovideoconvert here: on Windows/Wine it may pick d3d11/gl interop
                    // elements (d3d11upload/glcolorconvert/d3d11download) that fail to link.
                    let convert = ElementFactory::make("videoconvert").build()?;
                    let scale = ElementFactory::make("videoscale").build()?;

                    #[allow(unused_assignments)]
                    let mut video_appsink_caps = VideoCapsBuilder::new().build();
                    if cfg!(target_os = "android") {
                        #[cfg(feature = "gl")]
                        {
                            video_appsink_caps = VideoCapsBuilder::new()
                                .features([gstreamer_gl::CAPS_FEATURE_MEMORY_GL_MEMORY])
                                .field("texture-target", "2D")
                                //.framerate((60, 1).into())
                                .format(VideoFormat::Rgba)
                                .width(video_info.width() as i32)
                                .height(video_info.height() as i32)
                                .build()
                        }
                    } else {
                        video_appsink_caps = VideoCapsBuilder::new()
                            .format(video_info.format())
                            .width(video_info.width() as i32)
                            .height(video_info.height() as i32)
                            .build() //  video_info.to_caps()?;
                    };
                    let appsink = if low_latency {
                        AppSink::builder()
                            .name(id.clone() + "_sink_video")
                            .drop(true)
                            .max_buffers(1)
                            .sync(false)
                            .caps(&video_appsink_caps)
                            .build()
                    } else {
                        AppSink::builder()
                            .name(id.clone() + "_sink_video")
                            .drop(true)
                            //.max_buffers(1)
                            .caps(&video_appsink_caps)
                            .build()
                    };

                    if cfg!(target_os = "android") {
                        let glcolorconvert = ElementFactory::make("glcolorconvert").build()?;
                        let elements = vec![
                            &queue,
                            &glcolorconvert, //
                            &convert,
                            appsink.upcast_ref(),
                        ];
                        add_and_link(elements, pipeline.as_ref())?;
                    } else {
                        let capsfilter =
                            ElementFactory::make("capsfilter").property("caps", &video_appsink_caps).build()?;
                        let elements = vec![&queue, &convert, &scale, &capsfilter, appsink.upcast_ref()];
                        add_and_link(elements, pipeline.as_ref())?;
                    }

                    // Get the queue element's sink pad and link the decodebin's newly created
                    // src pad for the video stream to it.
                    let sink_pad = queue.static_pad("sink").expect("queue has no sinkpad");
                    src_pad.link(&sink_pad)?;

                    let video_tex = Tex::find(&tex_id)?;
                    Video1::set_video_callback(&appsink, video_tex, video_info)?;
                }
                Ok(())
            };
            if let Err(err) = insert_sink(is_audio, is_video) {
                Log::err(format!("Failed to insert sink : {:?}", err));
            }
        });
        Ok(())
    }

    fn init_player(&mut self) -> Result<(String, Pipeline), anyhow::Error> {
        // let mut video_tex = Tex::from_file(
        //     "textures/4Kscreen.png", //
        //     true,
        //     None,
        // )
        // .unwrap_or_default();
        let mut video_tex = Tex::gen_color(
            stereokit_rust::util::named_colors::WHITE,
            self.width,
            self.height,
            stereokit_rust::tex::TexType::Rendertarget,
            stereokit_rust::tex::TexFormat::RGBA32,
        );
        //let mut video_tex = Tex::render_target(self.width as usize, self.height as usize, None, None, None)?;
        let tex_id = self.repo.id_texture.clone();
        let material_id = self.id.clone() + "material_video";
        video_tex.id(&tex_id).sample_mode(TexSample::Linear);
        self.video_material.id(&material_id).diffuse_tex(&video_tex);

        let pipeline = Pipeline::default();
        Ok((tex_id, pipeline))
    }

    /// Getting data out of the appsink is done by setting callbacks on it.
    /// The appsink will then call those handlers, as soon as data is available.
    fn set_audio_callback(
        appsink: AppSink,
        sound_left: Sound,
        sound_right: Sound,
        audio_info: AudioInfo,
    ) -> Result<(), anyhow::Error> {
        appsink.set_callbacks(
            AppSinkCallbacks::builder()
                // Add a handler to the "new-sample" signal.
                .new_sample(move |appsink| {
                    // Pull the sample in question out of the appsink's buffer.
                    let sample = appsink.pull_sample().map_err(|_| gstreamer::FlowError::Eos)?;
                    let buffer = sample.buffer().ok_or_else(|| {
                        element_error!(
                            appsink,
                            gstreamer::ResourceError::Failed,
                            ("Failed to get buffer from appsink")
                        );
                        gstreamer::FlowError::Error
                    })?;
                    if false {
                        match AudioBufferRef::from_buffer_ref_readable(buffer, &audio_info) {
                            Ok(audio_buffer_ref) => {
                                if audio_buffer_ref.n_planes() > 1 {
                                    match audio_buffer_ref.plane_data(0) {
                                        Ok(samples) => {
                                            let f32_samples = samples.as_slice_of::<f32>().unwrap();
                                            sound_left.write_samples(f32_samples, Some((f32_samples.len()) as u64))
                                        }
                                        Err(bool_err) => element_error!(
                                            appsink,
                                            gstreamer::ResourceError::Failed,
                                            ("Failed to get left plane_data from audio_buffer: {}", bool_err)
                                        ),
                                    }
                                    match audio_buffer_ref.plane_data(1) {
                                        Ok(samples) => {
                                            let f32_samples = samples.as_slice_of::<f32>().unwrap();
                                            sound_right.write_samples(f32_samples, Some((f32_samples.len()) as u64))
                                        }
                                        Err(bool_err) => element_error!(
                                            appsink,
                                            gstreamer::ResourceError::Failed,
                                            ("Failed to get right plane_data from audio_buffer: {}", bool_err)
                                        ),
                                    }
                                } else {
                                    match audio_buffer_ref.plane_data(0) {
                                        Ok(samples) => {
                                            let f32_samples = samples.as_slice_of::<f32>().unwrap();
                                            let (left, right) = f32_samples.split_at(f32_samples.len() / 2);
                                            sound_left.write_samples(left, Some(left.len() as u64));
                                            sound_right.write_samples(right, Some(right.len() as u64));
                                        }
                                        Err(bool_err) => element_error!(
                                            appsink,
                                            gstreamer::ResourceError::Failed,
                                            ("Failed to get left plane_data from audio_buffer: {}", bool_err)
                                        ),
                                    }
                                }
                            }
                            Err(bool_err) => {
                                if false {
                                    element_error!(
                                        appsink,
                                        gstreamer::ResourceError::Failed,
                                        ("Failed to get audio_buffer from buffer: {}", bool_err)
                                    )
                                }
                            }
                        }
                    } else {
                        // // At this point, buffer is only a reference to an existing memory region somewhere.
                        // // When we want to access its content, we have to map it while requesting the required
                        // // mode of access (read, read/write).
                        // // This type of abstraction is necessary, because the buffer in question might not be
                        // // on the machine's main memory itself, but rather in the GPU's memory.
                        // // So mapping the buffer makes the underlying memory region accessible to us.
                        // // See: https://gstreamer.freedesktop.org/documentation/plugin-development/advanced/allocation.html
                        let map = buffer.map_readable().map_err(|_| {
                            element_error!(
                                appsink,
                                gstreamer::ResourceError::Failed,
                                ("Failed to map Audio buffer readable")
                            );
                            gstreamer::FlowError::Error
                        })?;
                        // We know what format the data in the memory region has, since we requested
                        // it by setting the appsink's caps. So what we do here is interpret the
                        // memory region we mapped as an array of signed 16 bit integers.
                        let sample = map.as_slice_of::<f32>().map_err(|_| {
                            element_error!(
                                appsink,
                                gstreamer::ResourceError::Failed,
                                ("Failed to interpret buffer as f32")
                            );
                            gstreamer::FlowError::Error
                        })?;
                        let sample_size = sample.len() / 2;
                        let (r, l) = sample.split_at(sample_size);
                        sound_right.write_samples(r, Some(sample_size as u64));
                        sound_left.write_samples(l, Some(sample_size as u64));
                    }

                    Ok(gstreamer::FlowSuccess::Ok)
                })
                .build(),
        );
        Ok(())
    }

    /// Getting data out of the appsink is done by setting callbacks on it.
    /// The appsink will then call those handlers, as soon as data is available.
    fn set_video_callback(appsink: &AppSink, mut video_tex: Tex, video_info: VideoInfo) -> Result<(), anyhow::Error> {
        const NB_FRAMES: u32 = 60;
        let mut frame_counter = 0u32;
        let mut previous_frame = 0f64;
        let mut frames_total = 0f64;

        let mut previous_timer = 0f64;

        appsink.set_callbacks(
            AppSinkCallbacks::builder()
                // Add a handler to the "new-sample" signal.
                .new_sample(move |appsink| {
                    let frame_err = |err| {
                        element_error!(
                            appsink,
                            gstreamer::ResourceError::Failed,
                            ("Failed to get VideoFrame from buffer: {}", err)
                        );
                        gstreamer::FlowError::Error
                    };
                    // get the last frame in channel
                    let sample = appsink.pull_sample().map_err(frame_err)?;
                    let buffer = sample.buffer_owned().unwrap();
                    let info = sample.caps().and_then(|caps| VideoInfo::from_caps(caps).ok()).unwrap();
                    let index = info.n_planes() - 1;

                    // if cfg!(target_os = "android") {
                    //     #[cfg(feature = "gl")]
                    //     {
                    //         use gstreamer::glib::BoolError;
                    //         use gstreamer_sys::{gst_buffer_map, gst_buffer_unmap};
                    //         use std::mem::MaybeUninit;

                    //         let hack = true;
                    //         let format = 32859; //frame.texture_format(index).map_err(|err| frame_err(err))?;

                    //         if hack {
                    //             // This code is missing from the gstreamer rust bindings as of 2023-July

                    //             // the buffer isn't guaranteed to live on GPU until we map it using MAP_GL
                    //             let mut map = {
                    //                 let mut map_info = MaybeUninit::uninit();
                    //                 unsafe {
                    //                     if 0 != gst_buffer_map(
                    //                         buffer.as_mut_ptr(),
                    //                         map_info.as_mut_ptr(),
                    //                         gstreamer::ffi::GST_MAP_READ | gstreamer_gl::ffi::GST_MAP_GL as u32,
                    //                     ) {
                    //                         map_info.assume_init()
                    //                     } else {
                    //                         return Err(frame_err(BoolError::new(
                    //                             "gst_buffer_map is 0".to_string(),
                    //                             "",
                    //                             "buffer",
                    //                             0,
                    //                         )));
                    //                     }
                    //                 }
                    //             };
                    //             Log::diag("¤¤¤¤¤¤¤¤¤¤¤¤¤¤ new video sample 2");
                    //             unsafe { gst_buffer_unmap(buffer.as_mut_ptr(), &mut map as *mut _) }
                    //             Log::diag("¤¤¤¤¤¤¤¤¤¤¤¤¤¤ new video sample 3");

                    //             let gl_mem = {
                    //                 if 0 == buffer.n_memory() {
                    //                     return Err(frame_err(BoolError::new(
                    //                         "n_memory is 0".to_string(),
                    //                         "",
                    //                         "buffer",
                    //                         0,
                    //                     )));
                    //                 }
                    //                 Log::diag("¤¤¤¤¤¤¤¤¤¤¤¤¤¤ new video sample 4");

                    //                 buffer.peek_memory(0).downcast_memory_ref::<GLMemory>().ok_or(frame_err(
                    //                     BoolError::new("n_memory is <= 0".to_string(), "", "buffer", 0),
                    //                 ))?
                    //             };
                    //             Log::diag("¤¤¤¤¤¤¤¤¤¤¤¤¤¤ new video sample 6");

                    //             let tex_id = gl_mem.texture_id();
                    //             let gl_target = gl_mem.texture_target().to_gl();
                    //             let width = info.width().try_into().unwrap();
                    //             let height = info.height().try_into().unwrap();
                    //             video_tex.set_native_surface(
                    //                 tex_id as *mut core::ffi::c_void,
                    //                 TexType::Rendertarget,
                    //                 format,
                    //                 width,
                    //                 height,
                    //                 1,
                    //                 true,
                    //             );
                    //             Log::diag(format!("texture: {}/{} {} {} {}", tex_id, gl_target, format, width, height));
                    //         } else {
                    //             let frame = GLVideoFrame::from_buffer_readable(buffer, &info).map_err(|err| {
                    //                 frame_err(BoolError::new(
                    //                     format!("no GLVideoFrame from buffer: {:?}", err),
                    //                     "",
                    //                     "buffer",
                    //                     0,
                    //                 ))
                    //             })?;
                    //             Log::diag("¤¤¤¤¤¤¤¤¤¤¤¤¤¤ new video sample 2");
                    //             // let sync_meta = frame.buffer().get_meta::<GLSyncMeta>().unwrap();
                    //             // sync_meta.wait(&app.shared_context);
                    //             let tex_id = frame.texture_id(index).map_err(frame_err)?;
                    //             Log::diag("¤¤¤¤¤¤¤¤¤¤¤¤¤¤ new video sample 3");

                    //             let width = frame.texture_width(index).map_err(frame_err)?;
                    //             Log::diag("¤¤¤¤¤¤¤¤¤¤¤¤¤¤ new video sample 4");

                    //             let height = frame.texture_height(index).map_err(frame_err)?;
                    //             Log::diag("¤¤¤¤¤¤¤¤¤¤¤¤¤¤ new video sample 5");

                    //             video_tex.set_native_surface(
                    //                 tex_id as *mut core::ffi::c_void,
                    //                 TexType::Rendertarget,
                    //                 format,
                    //                 width,
                    //                 height,
                    //                 1,
                    //                 true,
                    //             );
                    //             Log::diag(format!("texture: {} {} {} {}", tex_id, format, width, height));
                    //         }
                    //     }
                    // } else
                    // Log::info(format!(
                    //     "pts_or_dts:{:?} pts:{:?} dts:{:?} duration:{:?}",
                    //     &buffer.dts_or_pts(), //
                    //     &buffer.pts(),
                    //     &buffer.dts(),
                    //     &buffer.duration(),
                    // ));

                    if let Some(pts) = &buffer.dts_or_pts() {
                        frames_total += pts.seconds_f64() - previous_frame;
                        frame_counter += 1;
                        if frame_counter >= NB_FRAMES {
                            let timer = Time::get_total_unscaled();
                            Log::info(format!(
                                "fps: {} duration:{:?} real_fps: {}",
                                (NB_FRAMES as f64 / frames_total).round(),
                                &buffer.duration(),
                                (NB_FRAMES as f64 / (timer - previous_timer)).round()
                            ));
                            frame_counter = 0;
                            frames_total = 0f64;
                            previous_timer = timer;
                        }
                        previous_frame = pts.seconds_f64();
                    }

                    if let Ok(frame) = VideoFrame::from_buffer_readable(buffer, &info) {
                        let data = frame.plane_data(index).map_err(frame_err)?;
                        unsafe {
                            video_tex.set_colors(
                                video_info.width() as usize,
                                video_info.height() as usize,
                                data.as_ptr() as gpointer,
                            );
                        }
                    }

                    Ok(gstreamer::FlowSuccess::Ok)
                })
                .new_preroll(|_appsink| {
                    Log::diag(">>>>>new_preroll");
                    Ok(gstreamer::FlowSuccess::Ok)
                })
                .propose_allocation(|_appsink, allocation| {
                    Log::diag(format!(">>>>>propose_allocation {:?}", allocation));
                    true
                })
                .eos(|_appsink| {
                    Log::diag(">>>>>eos");
                })
                .build(),
        );
        Ok(())
    }

    fn bus_report(&mut self, pipeline: &Pipeline) -> bool {
        let id = self.id.clone();
        let stream_running = self.stream_running.clone();

        let get_element_name = |msg: &MessageRef| match msg.src() {
            Some(element) => element.name(),
            None => "<No Element>".into(),
        };

        let pipeline_weak = pipeline.downgrade();
        if let Some(bus) = pipeline.bus() {
            let bus_thread = thread::spawn(move || {
                use gstreamer::MessageView;
                let Some(pipeline) = pipeline_weak.upgrade() else {
                    Log::err("Unable to get pipleline !! ");
                    return false;
                };

                loop {
                    if let Some(message) = bus.pop() {
                        match message.view() {
                            MessageView::Eos(..) => {
                                Log::diag(format!("EOS on {} !", id));
                                if let Err(err) = pipeline.set_state(gstreamer::State::Ready) {
                                    Log::err(format!("Error when closing pipeline : {:?}", err));
                                }
                                if let Err(err) = pipeline.set_state(gstreamer::State::Null) {
                                    Log::err(format!("Error when closing pipeline : {:?}", err));
                                }
                                stream_running.store(false, Ordering::SeqCst);
                                break false;
                            }
                            MessageView::Error(err) => {
                                Log::err(format!(
                                    "Error from {:?}: {} ({:?})",
                                    err.src().map(|s| s.path_string()),
                                    err.error(),
                                    err.debug()
                                ));
                                if let Err(err) = pipeline.set_state(gstreamer::State::Ready) {
                                    Log::err(format!("Error when closing pipeline : {:?}", err));
                                }
                                if let Err(err) = pipeline.set_state(gstreamer::State::Null) {
                                    Log::err(format!("Error when closing pipeline : {:?}", err));
                                }
                                stream_running.store(false, Ordering::SeqCst);
                                break false;
                            }
                            MessageView::Warning(warning) => {
                                Log::warn(format!(
                                    "Warning from {:?}: {} ({:?})",
                                    warning.src().map(|s| s.path_string()),
                                    warning.error(),
                                    warning.debug()
                                ));
                            }
                            MessageView::Info(info) => {
                                let element = get_element_name(info.message());
                                Log::diag(format!("Info on {}/{:?} -> {:?}", id, element, info.message()));
                            }
                            MessageView::StateChanged(s) => {
                                let element = get_element_name(s.message());
                                Log::diag(format!("{:?} on {}/{} !", s.current(), id, element));
                            }
                            MessageView::StreamStart(s) => {
                                let element = get_element_name(s.message());
                                Log::diag(format!("StreamStart on {}/{} !", id, element));
                            }
                            MessageView::Latency(_l) => {
                                let err = pipeline.recalculate_latency();
                                Log::diag(format!("Latency on {} -> {:?} !", id, err));
                            }
                            MessageView::Qos(_qos) => {
                                // Log::diag(format!("Qos: {:?} on {}", qos, id));
                            }
                            MessageView::ClockLost(clock_lost) => {
                                Log::diag(format!("ClockLost: {:?} on {}", clock_lost, id));
                                let _err = pipeline.set_state(gstreamer::State::Paused);
                                let _err = pipeline.set_state(gstreamer::State::Playing);
                            }
                            MessageView::Buffering(buffering) => {
                                Log::diag(format!("Buffering: {:?} on {}", buffering, id));
                            }
                            // MessageView::StreamCollection(coll) => {
                            //     let element = get_element_name(coll.message());
                            //     let collection = coll.stream_collection();

                            //     Log::diag(format!("Collection from {:?} on {}/{} !", collection.name(), id, element));
                            // }
                            // otherwise => Log::diag(format!("Message {:?}", otherwise)),
                            _ => (),
                        };
                    } else {
                        sleep(Duration::from_millis(100));
                        if !stream_running.load(Ordering::Relaxed) {
                            Log::diag(format!("Closing Bus thread for {}", id));
                            if let Err(err) = pipeline.set_state(gstreamer::State::Ready) {
                                Log::err(format!("Error when closing pipeline : {:?}", err));
                            }
                            if let Err(err) = pipeline.set_state(gstreamer::State::Null) {
                                Log::err(format!("Error when closing pipeline : {:?}", err));
                            }
                            break false;
                        }
                    }
                }
            });
            self.bus_thread = Some(bus_thread);
            true
        } else {
            // unlikely
            Log::err(format!("Unable to get a bus for {}", id));
            false
        }
    }

    /// Called from IStepper::shutdown(triggering) then IStepper::shutdown_done(waiting for true response),
    /// here you can close your resources
    fn close(&mut self, triggering: bool) -> bool {
        if triggering {
            self.close_pipeline();
            self.shutdown_completed = false;
        } else {
            self.shutdown_completed = if let Some(thread) = &self.bus_thread {
                // if thread still alive ?
                thread.is_finished()
            } else {
                true
            }
        }
        self.shutdown_completed
    }
}

fn add_and_link(elements: Vec<&Element>, bin: &Bin) -> Result<(), anyhow::Error> {
    let elements = elements.as_slice();
    bin.add_many(elements)?;
    Element::link_many(elements)?;
    for e in elements {
        e.sync_state_with_parent()?
    }
    Ok(())
}

pub fn gstreamer_init() -> Result<(), anyhow::Error> {
    #[cfg(not(target_os = "android"))]
    {
        gstreamer::init()?;
        gstreamer::log::set_default_threshold(gstreamer::DebugLevel::Warning);
    }
    #[cfg(target_os = "android")]
    {
        gstreamer::log::set_default_threshold(gstreamer::DebugLevel::Warning);
    }

    #[cfg(target_os = "android")]
    {
        let ctx = ndk_context::android_context();
        let vm = unsafe { jni::JavaVM::from_raw(ctx.vm() as _) }?;
        //let activity = unsafe { jni::objects::JObject::from_raw(ctx.context() as _) };
        let mut env = vm.attach_current_thread()?;

        let media_codec_list = env.new_object("android/media/MediaCodecList", "(I)V", &[0i32.into()])?;

        let omx_decode_list = vec!["video/avc", "video/hevc", "video/x-vnd.on2.vp8", "video/x-vnd.on2.vp9"];

        for str in omx_decode_list {
            let jstr = env.new_string(str)?;
            let video_format = env.call_static_method(
                "android/media/MediaFormat",
                "createVideoFormat",
                "(Ljava/lang/String;II)Landroid/media/MediaFormat;",
                &[(&jstr).into(), 800i32.into(), 600i32.into()],
            )?;

            let media_codec: jni::objects::JString = env
                .call_method(
                    &media_codec_list,
                    "findDecoderForFormat",
                    "(Landroid/media/MediaFormat;)Ljava/lang/String;",
                    &[video_format.borrow()],
                )?
                .l()?
                .into();

            // "OMX.qcom.video.decoder.avc",
            // "OMX.qcom.video.decoder.vp8",
            // "OMX.qcom.video.decoder.hevc",
            match env.get_string(&media_codec) {
                Result::Ok(codec) => {
                    let str_codec: String = codec.into();
                    Log::diag(format!("Codec for {} -> {}", str, str_codec));
                }
                Err(err) => {
                    Log::warn(format!("No codec for {} ->  {:?}", str, err));
                }
            };
        }

        let omx_decode_list = vec![
            "openh264",
            "vp8dec",
            "vp9dec",
            "gldownload",
            "autovideoconvert",
            "autoconvert",
            "amcviddec-omxqcomvideodecoderh263",
            "amcviddec-omxqcomvideodecoderavc",
            "amcviddec-omxqcomvideodecoderhevc",
            "amcviddec-omxqcomvideodecodermpeg2",
            "amcviddec-omxqcomvideodecodermpeg4",
            "amcviddec-omxqcomvideodecodervp8",
            "amcviddec-omxqcomvideodecodervp9",
        ];

        let registry = gstreamer::Registry::get();

        for plugin in registry.plugins() {
            Log::diag(format!("plugin : {:?}", plugin.plugin_name()));
        }

        for element in omx_decode_list {
            if let Some(_feature) = registry.lookup_feature(element) {
                // gstreamer::prelude::PluginFeatureExtManual::set_rank(&feature, gstreamer::Rank::PRIMARY);
                // registry.add_feature(&feature)?;
                Log::diag(format!("Feature {} exist !", element));
            } else {
                Log::warn(format!("Feature {} does not exist !", element));
            }
        }
    }
    Ok(())
}

/// Generate an hemispheric screen
pub fn generate_screen(radius: f32, screen_ratio: Vec2) -> Mesh {
    let mut verts: Vec<Vertex> = vec![];
    let mut inds: Vec<Inds> = vec![];

    let aspect_ratio = screen_ratio.x / screen_ratio.y;
    let res_u = screen_ratio.x;
    let res_v = screen_ratio.y;

    for j in 0..res_v as u32 {
        let v = j as f32 / (res_v - 1.0);
        for i in 0..res_u as u32 {
            let u = i as f32 / (res_u - 1.0);
            let x = radius * v.cos() * u.cos() * aspect_ratio;
            let z = radius * v.sin();
            let y = radius * v.cos() * u.sin();

            verts.push(Vertex::new(
                Vec3::new(x, y, z), //
                Vec3::FORWARD,
                Some(Vec2::new(u, v)),
                None,
            ));

            Log::diag(format!("vertex: {} {} {}", x, y, z));

            let res_u = res_u as u32;
            let res_v = res_v as u32;
            let last_line = j == res_v - 1;
            if !last_line {
                let row_is_even = i % 2 == 0;
                let last_row = i == res_u - 1;
                let a = j * res_u + i;
                let b = j * res_u + i + 1;
                let c = (j + 1) * res_u + i;
                if row_is_even {
                    if !last_row {
                        inds.push(a);
                        inds.push(b);
                        inds.push(c);
                        inds.push(a);
                        inds.push(c);
                        inds.push(b);
                    }
                } else {
                    let c_previous = (j + 1) * res_u + i - 1;
                    let c_following = (j + 1) * res_u + i + 1;
                    inds.push(a);
                    inds.push(c);
                    inds.push(c_previous);
                    inds.push(a);
                    inds.push(c_previous);
                    inds.push(c);
                    if !last_row {
                        inds.push(a);
                        inds.push(c_following);
                        inds.push(c);
                        inds.push(a);
                        inds.push(c);
                        inds.push(c_following);

                        inds.push(a);
                        inds.push(b);
                        inds.push(c_following);
                        inds.push(a);
                        inds.push(c_following);
                        inds.push(b);
                    }
                }
            }
        }
    }

    let mut mesh = Mesh::new();
    mesh.set_inds(inds.as_slice());
    mesh.set_verts(verts.as_slice(), true);

    mesh
}
