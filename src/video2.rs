use std::{cell::RefCell, rc::Rc};

use anyhow::Ok;
use byte_slice_cast::AsSliceOf;
use gstreamer::{
    element_error,
    glib::ffi::gpointer,
    prelude::{Cast, ElementExt, GstBinExtManual, GstObjectExt, ObjectExt, PadExt},
    Bin, Bus, ClockTime, Element, ElementFactory, GhostPad, MessageType,
};

use gstreamer_app::{AppSink, AppSinkCallbacks};

use gstreamer_audio::{AudioCapsBuilder, AUDIO_FORMAT_F32};
use gstreamer_gl::prelude::VideoOverlayExtManual;
use gstreamer_video::{VideoCapsBuilder, VideoFormat, VideoOverlay};
use stereokit_rust::{
    event_loop::{IStepper, StepperId},
    font::Font,
    material::Material,
    maths::{Matrix, Quat, Vec2, Vec3},
    mesh::Mesh,
    sk::{MainThreadToken, SkInfo},
    sound::{Sound, SoundInst},
    system::{Log, Text, TextStyle},
    tex::{Tex, TexFormat, TexSample, TexType},
    util::{
        named_colors::{RED, WHITE},
        Time,
    },
};

/// The video stepper
pub struct Video2 {
    id: StepperId,
    sk_info: Option<Rc<RefCell<SkInfo>>>,
    pub uri: String,
    pub v3_enabled: bool,
    pub width: i32,
    pub height: i32,
    pub transform_screen: Matrix,
    pub plane: Mesh,
    pub text: String,
    pub transform: Matrix,
    pub text_style: Option<TextStyle>,
    video_material: Material,
    pipeline: Option<Element>,
    bus: Option<Bus>,
    first: bool,
    stream_running: bool,
    sound_left: Sound,
    sound_left_id: String,
    sound_left_inst: Option<SoundInst>,
}

unsafe impl Send for Video2 {}

/// This code may be called in some threads, so no StereoKit code
impl Default for Video2 {
    fn default() -> Self {
        Self {
            id: "Video1".to_string(),
            sk_info: None,
            uri: "".into(),
            v3_enabled: true,
            width: 1920,
            height: 1080,
            transform_screen: Matrix::tr(&(Vec3::new(0.0, 1.0, -1.5)), &Quat::from_angles(90.0, 0.0, 0.0)),
            plane: Mesh::generate_plane_up(Vec2::new(1.920, 1.080), None, true),
            text: "Video1".to_owned(),
            transform: Matrix::tr(&(Vec3::new(0.0, 2.0, -2.5)), &Quat::from_angles(0.0, 180.0, 0.0)),
            text_style: Some(Text::make_style(Font::default(), 0.3, RED)),
            video_material: Material::unlit().copy(),
            pipeline: None,
            bus: None,
            first: true,
            stream_running: false,
            sound_left: Sound::click(),
            sound_left_id: "None".into(),
            sound_left_inst: None,
        }
    }
}

/// All the code here run in the main thread
impl IStepper for Video2 {
    fn initialize(&mut self, id: StepperId, sk_info: Rc<RefCell<SkInfo>>) -> bool {
        self.id = id;
        self.sk_info = Some(sk_info.clone());

        self.sound_left = Sound::create_stream(200.0).unwrap();
        self.sound_left_id = self.id.clone() + "left";
        self.sound_left.id(&self.sound_left_id);

        if let Err(error) = self.init_playbin() {
            Log::err(format!("Unable to initialize video : {:?}", error));
            false
        } else {
            self.stream_running = true;
            true
        }
    }

    fn step(&mut self, token: &MainThreadToken) {
        if let Some(pipeline) = &self.pipeline {
            if self.first {
                let _res = pipeline.set_state(gstreamer::State::Playing);
                self.first = false;
                self.sound_left_inst = Some(self.sound_left.play(self.transform_screen.get_pose().position, Some(1.0)));
            }

            if self.stream_running {
                self.check_bus();
            }
        }

        self.plane.draw(token, &self.video_material, self.transform_screen, None, None);
        Text::add_at(token, &self.text, self.transform, self.text_style, None, None, None, None, None, None);
    }

    fn shutdown(&mut self) {
        if let Some(pipeline) = &self.pipeline {
            if let Some(sound_inst) = self.sound_left_inst {
                sound_inst.stop()
            };
            Log::diag(format!("--->{} state {:?}", self.id, pipeline.state(ClockTime::from_mseconds(100))));
            Log::diag(format!("------> lockedstate {:?}", pipeline.is_locked_state()));
            match pipeline.set_state(gstreamer::State::Paused) {
                Err(err) => Log::err(format!("Error when pausing pipeline : {:?}", err)),
                _ => {
                    if let Err(err) = pipeline.set_state(gstreamer::State::Null) {
                        Log::err(format!("Error when closing pipeline : {:?}", err));
                    }
                }
            }
        }
        self.bus = None;
        self.pipeline = None;
        Log::diag(format!("Closing Video1/{} !!!", self.id));
    }
}

impl Video2 {
    /// Create the video player
    pub fn new(uri: String, v3_enabled: bool) -> Self {
        Self { uri, v3_enabled, ..Default::default() }
    }

    /// init a video playbin
    ///
    fn init_playbin(&mut self) -> Result<(), anyhow::Error> {
        let tex_id = self.init_player()?;
        let playbin = if self.v3_enabled {
            ElementFactory::make("playbin3").property("uri", &self.uri).build()?
        } else {
            ElementFactory::make("playbin").property("uri", &self.uri).build()?
        };

        //--- audio
        let equalizer = ElementFactory::make("equalizer-3bands").build()?;
        let a_convert = ElementFactory::make("audioconvert").build()?;
        let resample = ElementFactory::make("audioresample").build()?;
        let audio_appsink = AppSink::builder()
            .caps(&AudioCapsBuilder::new_interleaved().format(AUDIO_FORMAT_F32).channels(1).build())
            .build();

        let audio_bin = Bin::with_name("audio_sink_bin");

        let elements = &[&equalizer, &a_convert, &resample, audio_appsink.upcast_ref()];
        audio_bin.add_many(elements)?;
        Element::link_many(elements)?;
        for e in elements {
            e.sync_state_with_parent()?;
        }

        if let Some(pad) = equalizer.static_pad("sink") {
            let ghost_pad = GhostPad::with_target(&pad)?;
            ghost_pad.set_active(true)?;
            audio_bin.add_pad(&ghost_pad)?;
        }

        equalizer.set_property("band1", -24.0);
        equalizer.set_property("band2", -24.0);

        let sound_left = Sound::find(&self.sound_left_id)?;
        Video2::set_audio_callback(audio_appsink, sound_left);

        playbin.set_property("audio-sink", audio_bin);

        //--- video
        let v_convert = ElementFactory::make("videoconvert").build()?;
        let scale = ElementFactory::make("videoscale").build()?;
        let video_appsink = AppSink::builder()
            .caps(&VideoCapsBuilder::new().format(VideoFormat::Rgba).width(self.width).height(self.height).build())
            .build();

        let video_bin = Bin::with_name("video_sink_bin");

        let elements = &[&v_convert, &scale, video_appsink.upcast_ref()];
        video_bin.add_many(elements)?;
        Element::link_many(elements)?;
        for e in elements {
            e.sync_state_with_parent()?
        }

        if let Some(pad) = v_convert.static_pad("sink") {
            let ghost_pad = GhostPad::with_target(&pad)?;
            ghost_pad.set_active(true)?;
            video_bin.add_pad(&ghost_pad)?;
        }

        let video_tex = Tex::find(&tex_id)?;

        if false {
            //cfg!(target_os = "android") {
            let play_binbin = playbin.clone().dynamic_cast::<VideoOverlay>().unwrap();
            let native = video_tex.get_native_surface() as usize;
            unsafe { play_binbin.set_window_handle(native) };
        } else {
            Video2::set_video_callback(video_appsink, video_tex, self.width as usize, self.height as usize);
            playbin.set_property("video-sink", video_bin);
        }
        //--- A bus and we can run
        self.bus = Some(playbin.bus().expect("Pipeline without bus. Shouldn't happen!"));
        self.pipeline = Some(playbin);

        Ok(())
    }

    fn init_player(&mut self) -> Result<String, anyhow::Error> {
        let mut video_tex = Tex::gen_color(WHITE, self.width, self.height, TexType::Rendertarget, TexFormat::RGBA32);
        //let mut video_tex = Tex::render_target(self.width, self.height, None, None, None)?;
        let tex_id = self.id.clone() + "video";
        let material_id = self.id.clone() + "material_video";
        video_tex.id(&tex_id).sample_mode(TexSample::Point);
        self.video_material.id(&material_id).diffuse_tex(&video_tex);

        Ok(tex_id)
    }

    /// Getting data out of the appsink is done by setting callbacks on it.
    /// The appsink will then call those handlers, as soon as data is available.
    fn set_audio_callback(appsink: AppSink, sound_left: Sound) {
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

                    // At this point, buffer is only a reference to an existing memory region somewhere.
                    // When we want to access its content, we have to map it while requesting the required
                    // mode of access (read, read/write).
                    // This type of abstraction is necessary, because the buffer in question might not be
                    // on the machine's main memory itself, but rather in the GPU's memory.
                    // So mapping the buffer makes the underlying memory region accessible to us.
                    // See: https://gstreamer.freedesktop.org/documentation/plugin-development/advanced/allocation.html
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
                    let samples = map.as_slice_of::<f32>().map_err(|_| {
                        element_error!(
                            appsink,
                            gstreamer::ResourceError::Failed,
                            ("Failed to interpret buffer as f32")
                        );

                        gstreamer::FlowError::Error
                    })?;

                    sound_left.write_samples(samples.as_ptr(), samples.len() as u64);
                    Result::<gstreamer::FlowSuccess, gstreamer::FlowError>::Ok(gstreamer::FlowSuccess::Ok)
                })
                .build(),
        );
    }

    /// Getting data out of the appsink is done by setting callbacks on it.
    /// The appsink will then call those handlers, as soon as data is available.
    fn set_video_callback(appsink: AppSink, mut video_tex: Tex, width: usize, height: usize) {
        let mut timestamp = 0.0;
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

                    // At this point, buffer is only a reference to an existing memory region somewhere.
                    // When we want to access its content, we have to map it while requesting the required
                    // mode of access (read, read/write).
                    // This type of abstraction is necessary, because the buffer in question might not be
                    // on the machine's main memory itself, but rather in the GPU's memory.
                    // So mapping the buffer makes the underlying memory region accessible to us.
                    // See: https://gstreamer.freedesktop.org/documentation/plugin-development/advanced/allocation.html
                    let map = buffer.map_readable().map_err(|_| {
                        element_error!(
                            appsink,
                            gstreamer::ResourceError::Failed,
                            ("Failed to map Audio buffer readable")
                        );

                        gstreamer::FlowError::Error
                    })?;

                    let fps = 1.0 / (timestamp - Time::get_total_unscaledf());
                    timestamp = Time::get_total_unscaledf();
                    Log::diag(format!("fps : {:.0}", fps));

                    video_tex.set_colors(width, height, map.as_ptr() as gpointer);

                    Result::<gstreamer::FlowSuccess, gstreamer::FlowError>::Ok(gstreamer::FlowSuccess::Ok)
                })
                .build(),
        );
    }

    fn check_bus(&mut self) {
        if let Some(bus) = &self.bus {
            if let Some(msg) = bus.timed_pop_filtered(
                ClockTime::from_mseconds(1), //ClockTime::MAX,
                &[MessageType::Error, MessageType::Eos, MessageType::StateChanged],
            ) {
                use gstreamer::MessageView;

                match msg.view() {
                    MessageView::Eos(..) => {
                        if let Some(element) = msg.src() {
                            if let Some(pipeline) = &self.pipeline {
                                if element == pipeline {
                                    pipeline
                                        .set_state(gstreamer::State::Null)
                                        .expect("Unable to set the pipeline to the `Null` state");
                                    Log::diag(format!("EOS on {} !", self.id));
                                    self.stream_running = false;
                                    self.bus = None;
                                    self.pipeline = None;
                                }
                            }
                        }
                    }
                    MessageView::Error(err) => {
                        if let Some(element) = msg.src() {
                            if let Some(pipeline) = &self.pipeline {
                                if element == pipeline {
                                    pipeline
                                        .set_state(gstreamer::State::Null)
                                        .expect("Unable to set the pipeline to the `Null` state");

                                    self.stream_running = false;
                                    self.bus = None;
                                    self.pipeline = None;
                                }
                                Log::err(format!("Error on {} : {:?} -> {:?}", self.id, element.name(), err.message()));
                            }
                        }
                    }
                    MessageView::Warning(warning) => {
                        if let Some(element) = msg.src() {
                            Log::warn(format!(
                                "Warning on {} : {:?} -> {:?}",
                                self.id,
                                element.name(),
                                warning.message()
                            ));
                        }
                    }
                    MessageView::Info(info) => {
                        if let Some(element) = msg.src() {
                            Log::diag(format!("Info on {} : {:?} -> {:?}", self.id, element.name(), info.message()));
                        }
                    }
                    MessageView::StateChanged(s) => {
                        if let Some(element) = msg.src() {
                            if let Some(pipeline) = &self.pipeline {
                                if element == pipeline {
                                    if s.current() == gstreamer::State::Playing {
                                        Log::info(format!("PLAYING {} !", self.id));
                                    } else {
                                        Log::diag(format!("{:?} on {} !", s.current(), self.id));
                                    }
                                }
                            }
                        }
                    }
                    _ => (),
                }
            }
        }
    }
}

pub fn gstreamer_init() -> Result<(), anyhow::Error> {
    gstreamer::log::set_default_threshold(gstreamer::DebugLevel::Warning);
    #[cfg(not(target_os = "android"))]
    {
        gstreamer::init()?;
    }

    // #[cfg(target_os = "android")]
    // {
    //     let ctx = ndk_context::android_context();
    //     let vm = unsafe { jni::JavaVM::from_raw(ctx.vm() as _) }?;
    //     //let activity = unsafe { jni::objects::JObject::from_raw(ctx.context() as _) };
    //     let mut env = vm.attach_current_thread()?;

    //     let media_codec_list = env.new_object("android/media/MediaCodecList", "(I)V", &[0i32.into()])?;

    //     // let media_codecs: jni::objects::JObjectArray = env
    //     //     .call_method(&media_codec_list, "getCodecInfos", "()[Landroid/media/MediaCodecInfo;", &[])?
    //     //     .l()?
    //     //     .into();
    //     // Log::diag(format!("le truc : {:?}", media_codecs));

    //     let omx_decode_list = vec!["video/avc", "video/hevc", "video/x-vnd.on2.vp8", "video/x-vnd.on2.vp9"];

    //     for str in omx_decode_list {
    //         let jstr = env.new_string(str)?;
    //         let video_format = env.call_static_method(
    //             "android/media/MediaFormat",
    //             "createVideoFormat",
    //             "(Ljava/lang/String;II)Landroid/media/MediaFormat;",
    //             &[(&jstr).into(), 800i32.into(), 600i32.into()],
    //         )?;

    //         let media_codec: jni::objects::JString = env
    //             .call_method(
    //                 &media_codec_list,
    //                 "findDecoderForFormat",
    //                 "(Landroid/media/MediaFormat;)Ljava/lang/String;",
    //                 &[video_format.borrow()],
    //             )?
    //             .l()?
    //             .into();

    //         // "OMX.qcom.video.decoder.avc",
    //         // "OMX.qcom.video.decoder.vp8",
    //         // "OMX.qcom.video.decoder.hevc",
    //         match env.get_string(&media_codec) {
    //             Result::Ok(codec) => {
    //                 let str_codec: String = codec.into();
    //                 Log::diag(format!("Codec for {} -> {}", str, str_codec));
    //             }
    //             Err(err) => {
    //                 Log::warn(format!("No codec for {} ->  {:?}", str, err));
    //             }
    //         };
    //     }

    //     let omx_decode_list = vec![
    //         "openh264",
    //         "vp8dec",
    //         "vp9dec",
    //         "amcviddec-c2qtiavcdecoder",
    //         "amcviddec-omxqcomvideodecoderh263",
    //         "amcviddec-omxqcomvideodecoderavc",
    //         "amcviddec-omxqcomvideodecoderhevc",
    //         "amcviddec-omxqcomvideodecodermpeg2",
    //         "amcviddec-omxqcomvideodecodermpeg4",
    //         "amcviddec-omxqcomvideodecodervp8",
    //         "amcviddec-omxqcomvideodecodervp9",
    //     ];

    //     let registry = gstreamer::Registry::get();

    //     for plugin in registry.plugins() {
    //         Log::diag(format!("plugin : {:?}", plugin.plugin_name()));

    //         // for feat in registry.features_by_plugin(&plugin.plugin_name()) {
    //         //Log::diag(format!("   feature : {:?}", feat));
    //         // }
    //     }

    //     for element in omx_decode_list {
    //         if let Some(feature) = registry.lookup_feature(element) {
    //             gstreamer::prelude::PluginFeatureExtManual::set_rank(&feature, gstreamer::Rank::PRIMARY);
    //             registry.add_feature(&feature)?;
    //         } else {
    //             Log::warn(format!("Feature {} does not exist !", element));
    //         }
    //     }
    // }
    Ok(())
}
