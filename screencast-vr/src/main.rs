use anyhow::bail;

use gstreamer_rtsp_server::prelude::{RTSPMediaFactoryExt, RTSPMountPointsExt, RTSPServerExt, RTSPServerExtManual};

use std::fs;
use std::path::PathBuf;
use std::{collections::HashMap, os::fd::AsRawFd, thread};

use eframe::egui::{self};
use futures_util::stream::StreamExt;

use gstreamer::{glib::MainLoop, prelude::*, MessageRef};

use zbus::{
    proxy,
    zvariant::{DeserializeDict, Fd, ObjectPath, OwnedObjectPath, SerializeDict, Str, Structure, Type, Value},
    Connection,
};
fn main() -> Result<(), eframe::Error> {
    env_logger::init(); // Log to stderr (if you run with `RUST_LOG=debug`).
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([320.0, 240.0]),
        ..Default::default()
    };
    let desktop = if cfg!(target_os = "linux") {
        match std::env::var("WAYLAND_DISPLAY") {
            Ok(_var) => Desktop::Wayland,
            Err(_) => Desktop::X11,
        }
    } else if cfg!(target_os = "windows") {
        Desktop::Windows
    } else {
        Desktop::MacOs
    };
    let _err = gstreamer::init();
    gstreamer::log::set_default_threshold(gstreamer::DebugLevel::Warning);
    //let boxe = Box::new(MyApp::new_wayland().await.unwrap());
    // let boxe =
    //     Box::new(async_io::block_on(MyApp::new_wayland()).expect("Error listening to signal"));
    eframe::run_native(
        "My egui App",
        options,
        Box::new(|cc| {
            // This gives us image support:
            egui_extras::install_image_loaders(&cc.egui_ctx);

            Ok(Box::new(MyApp::new(desktop)))
        }),
    )
}

#[derive(Debug, Clone, PartialEq)]
enum Desktop {
    Wayland,
    X11,
    Windows,
    MacOs,
}

#[derive(Debug, Clone, PartialEq)]
enum Coding {
    H264,
    H265,
    VP9,
}

#[derive(Debug, Clone, PartialEq)]
enum Encoder {
    Nvidia,
    NvidiaCuda,
    Raw,
}

pub const HOST: &str = "host=192.168.1.131 port=5000";
// pub const HOST: &str = "host=192.168.1.184 port=5000";

struct MyApp {
    desktop: Desktop,
    window_id: String,
    connexion: String,
    streams: Vec<std::thread::JoinHandle<()>>,
    coding: Coding,
    fps: u32,
    with_gpu: bool,
    with_cuda: bool,
    with_rtmp: bool,
    with_rtsp: bool,
    to_ip: String,
}
impl MyApp {
    fn new(desktop: Desktop) -> Self {
        Self {
            desktop,
            window_id: String::new(),
            connexion: "".into(),
            streams: vec![],
            coding: Coding::H264,
            fps: 60,
            with_gpu: true,
            with_cuda: true,
            with_rtsp: false,
            with_rtmp: false,
            to_ip: HOST.into(),
        }
    }

    fn launch_stream(&mut self, coding: Coding, encoder: Encoder) {
        let index = (self.streams.len() + 1).to_string();
        let desktop = self.desktop.clone();
        let with_rtsp = self.with_rtsp;
        let to_ip = self.to_ip.clone();
        let window_id = self.window_id.clone();
        let fps = self.fps;
        let thread = thread::spawn(move || {
            if let Ok(mut stream) =
                async_io::block_on(Self::stream(desktop, index, coding, encoder, to_ip, window_id, fps))
            {
                let err = if with_rtsp {
                    async_io::block_on(stream.main_loop_server())
                } else {
                    async_io::block_on(stream.main_loop())
                };
                println!("main loop returned with {:?}", err);
            }
        });

        self.streams.push(thread);
    }

    async fn stream(
        desktop: Desktop,
        index: String,
        coding: Coding,
        encoder: Encoder,
        to_ip: String,
        window_id: String,
        fps: u32,
    ) -> Result<Stream<'static>, anyhow::Error> {
        match desktop {
            Desktop::Wayland => Stream::new_wayland(index, coding, encoder, to_ip).await,
            Desktop::X11 => Stream::new_x11(coding, encoder, to_ip, window_id, fps).await,
            Desktop::Windows => todo!(),
            Desktop::MacOs => todo!(),
        }
    }
}

impl eframe::App for MyApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("XR ScreenCast");
            ui.horizontal(|ui| {
                let window_id_label = ui.label("endx endy:");
                ui.text_edit_singleline(&mut self.window_id).labelled_by(window_id_label.id);
            });
            ui.horizontal(|ui| {
                let to_ip = ui.label("cast to:");
                ui.text_edit_singleline(&mut self.to_ip).labelled_by(to_ip.id);
            });

            ui.horizontal(|ui| {
                ui.radio_value(&mut self.coding, Coding::H264, "H264");
                ui.radio_value(&mut self.coding, Coding::H265, "H265");
                ui.radio_value(&mut self.coding, Coding::VP9, "VP9");
            });
            ui.horizontal(|ui| {
                ui.checkbox(&mut self.with_gpu, "GPU");
                ui.checkbox(&mut self.with_cuda, "Cuda");
                ui.checkbox(&mut self.with_rtmp, "RTMP");
                ui.checkbox(&mut self.with_rtsp, "RTSP");
            });
            if self.with_cuda {
                self.with_gpu = true;
            }
            if !self.with_gpu {
                self.with_cuda = false;
            }
            ui.add(egui::Slider::new(&mut self.fps, 0..=120).text("fps"));
            if ui.button(format!("ScreenCast {:?}", self.coding)).clicked() {
                let encoder = if self.with_cuda {
                    Encoder::NvidiaCuda
                } else if self.with_gpu {
                    Encoder::Nvidia
                } else {
                    Encoder::Raw
                };
                self.launch_stream(self.coding.clone(), encoder);
            }

            ui.label(format!("Hello '{}', stream number {}", self.connexion, self.streams.len()));

            ui.image(egui::include_image!("../../res/mipmap-hdpi/app_icon.png"));
        });
    }
}

fn encoding_str(coding: Coding, encoder: Encoder, gst_str: &mut String) {
    match coding {
        Coding::H264 => match encoder {
            Encoder::Nvidia => {
                // nvautogpuh264enc - Auto GPU select mode with config-interval for inline SPS/PPS
                *gst_str +=
                    "! videoconvert ! nvautogpuh264enc bitrate=8192 gop-size=30 repeat-sequence-header=true aud=true \
                    ! h264parse config-interval=-1 \
                    ! video/x-h264,stream-format=byte-stream,alignment=au \
                    ! rtph264pay pt=96 config-interval=1 mtu=1200 ";
            }
            Encoder::NvidiaCuda => {
                // nvh264enc - CUDA mode encoder
                *gst_str += "! videoconvert ! nvh264enc bitrate=8192 rc-mode=vbr qp-const-b=0 qp-const-i=0 qp-const-p=0 gop-size=475 qos=true preset=low-latency-hq \
                    ! h264parse config-interval=-1 \
                    ! video/x-h264,stream-format=byte-stream,alignment=au,profile=baseline \
                    ! rtph264pay name=pay0 pt=96 config-interval=1 mtu=1200";
            }
            Encoder::Raw => {
                // gst_str += "! videoconvert ! rtpvrawpay ! application/x-rtp, media=video, encoding-name=RAW";
                *gst_str +=
                "! videoconvert ! video/x-raw,format=I420 ! x264enc bitrate=8192 speed-preset=superfast tune=zerolatency byte-stream=true sliced-threads=true ! h264parse config-interval=-1 ! video/x-h264,stream-format=byte-stream ! rtph264pay";
            }
        },
        Coding::H265 => match encoder {
            Encoder::Nvidia => {
                *gst_str +=
                    "! videoconvert ! nvautogpuh265enc bitrate=8192 gop-size=30 repeat-sequence-header=true aud=true \
                    ! h265parse config-interval=-1 \
                    ! video/x-h265,stream-format=byte-stream,alignment=au \
                    ! rtph265pay pt=96 config-interval=1 mtu=1200 ";
            }
            Encoder::NvidiaCuda => {
                *gst_str += "! videoconvert ! nvh265enc bitrate=8192 rc-mode=vbr qp-const-b=0 qp-const-i=0 qp-const-p=0 gop-size=475 qos=true preset=low-latency-hq \
                    ! h265parse config-interval=-1 \
                    ! video/x-h265,stream-format=byte-stream,alignment=au \
                    ! rtph265pay name=pay0 pt=96 config-interval=1 mtu=1200";
            }
            Encoder::Raw => {
                *gst_str += "! videoconvert ! x265enc speed-preset=superfast tune=zerolatency  ! rtph265pay";
            }
        },
        Coding::VP9 => match encoder {
            Encoder::Nvidia => {
                *gst_str +=
                    "! videoconvert ! vp9enc  ! vp9parse ! video/x-vp9,stream-format=byte-stream  !   rtpvp9pay ";
            }
            Encoder::NvidiaCuda => {
                *gst_str +=
                    "! videoconvert ! vp9enc  ! vp9parse ! video/x-vp9,stream-format=byte-stream  !   rtpvp9pay ";
            }
            Encoder::Raw => {
                *gst_str += "! videoconvert ! vp9enc ! rtpvp9pay";
            }
        },
    }
}

// match coding {
//     Coding::NvidiaH264 => {
//         //gst_str += "! videoconvert ! queue ! encodebin profile=\"video/x-h264,tune=zerolatency,profile=baseline\" !   rtph264pay ";
//         // Yes gst_str += "! videoconvert ! nvh264enc ! h264parse config-interval=-1 ! video/x-h264,stream-format=byte-stream  !   rtph264pay ";
//         gst_str += "! videoconvert ! nvh264enc  bitrate=2000 rc-mode=cbr gop-size=1 qos=true preset=low-latency-hq ! h264parse config-interval=-1 ! video/x-h264,stream-format=byte-stream  !   rtph264pay ";
//         // Yes: gst_str += "! videoconvert ! nvcudah264enc  repeat-sequence-header=true preset=p1 ! rtph264pay name=pay0";
//         //gst_str += "! videoconvert ! nvcudah264enc preset=p1 ! h264parse config-interval=-1 ! video/x-h264,stream-format=byte-stream ! rtph264pay name=pay0";
//         // Good: gst_str += "! videoconvert ! nvh264enc bitrate=2000 rc-mode=cbr gop-size=-1 qos=true preset=low-latency-hq  ! h264parse  config-interval=-1 ! video/x-h264,stream-format=byte-stream,payload=123  !   rtph264pay";
//         //gst_str += "! videoconvert ! nvh264enc bitrate=2000 rc-mode=cbr  qos=true preset=low-latency-hq ! h264parse  config-interval=-1 ! video/x-h264,stream-format=byte-stream,payload=123  !  rtph264pay name=pay0";
//     }
//     Coding::H264 => {
//         gst_str +=
//             "! videoconvert ! x264enc bitrate=2000 qos=true pass=pass1 speed-preset=superfast tune=zerolatency byte-stream=true sliced-threads=true bframes=1 ! rtph264pay name=pay0";
//         // bitrate=6000 pass=pass1 speed-preset=ultrafast tune=zerolatency sliced-threads=true threads=6
//     }
//     Coding::Raw => {
//         gst_str += "! videoconvert ! encodebin2 profile=\"video/x-raw\" ! rtpvrawpay ! application/x-rtp, media=video, encoding-name=RAW";
//     }
// }

#[derive(Debug)]
struct Stream<'a> {
    _desktop: Desktop,
    connexion: String,
    to_ip: String,
    _id: u32,
    _screencast_proxy: Option<ScreenCastProxy<'a>>,
    _conn: Option<Connection>,
    _pw_fd: Option<std::os::fd::OwnedFd>,
}
impl<'a> Stream<'a> {
    /// Obtenir le chemin du fichier de configuration
    fn config_path() -> PathBuf {
        let mut path = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
        path.push("screencast-vr");
        fs::create_dir_all(&path).ok();
        path.push("session_config.txt");
        path
    }

    /// Charger le restore_token depuis le fichier
    fn load_restore_token() -> Option<String> {
        let path = Self::config_path();
        fs::read_to_string(path).ok().and_then(|s| {
            let token = s.trim().to_string();
            if token.is_empty() {
                None
            } else {
                Some(token)
            }
        })
    }

    /// Sauvegarder le restore_token dans le fichier
    fn save_restore_token(token: &str) {
        let path = Self::config_path();
        if let Err(e) = fs::write(&path, token) {
            eprintln!("Failed to save restore_token to {:?}: {}", path, e);
        } else {
            println!("Saved restore_token to {:?}", path);
        }
    }

    /// ximagesrc ! videoconvert ! x264enc speed-preset=superfast tune=zerolatency byte-stream=true sliced-threads=true ! rtph264pay ! udpsink ".to_owned() + HOST + "
    pub async fn new_x11(
        coding: Coding,
        encoder: Encoder,
        to_ip: String,
        window_id: String,
        fps: u32,
    ) -> Result<Self, anyhow::Error> {
        let xid = if window_id.is_empty() { String::new() } else { format!("endx={} endy={}", window_id, window_id) };
        let mut gst_str: String = format!(
            "ximagesrc {} use-damage=false remote=1 blocksize=16384  ! video/x-raw, framerate={}/1 ! queue ",
            xid, fps
        );

        encoding_str(coding, encoder, &mut gst_str);

        // conn.add_match(, f)
        Ok(Self {
            _desktop: Desktop::X11,
            connexion: gst_str,
            to_ip,
            _id: 0,
            _screencast_proxy: None,
            _conn: None,
            _pw_fd: None,
        })
    }

    async fn new_wayland(
        session_handle_token: String,
        coding: Coding,
        encoder: Encoder,
        to_ip: String,
    ) -> Result<Self, anyhow::Error> {
        let conn = Connection::session().await?;

        let screencast_proxy = ScreenCastProxy::new(&conn).await?;

        println!("Available source type : {}", screencast_proxy.available_source_types().await?);
        println!("Available cursor mode : {}", screencast_proxy.available_cursor_modes().await?);

        // Charger le restore_token depuis le fichier s'il existe
        let restore_token = Self::load_restore_token();
        if let Some(ref token) = restore_token {
            println!("Using saved restore_token: {}", token);
        } else {
            println!("No restore_token found, will create new session");
        }

        //>>>>>> CreateSession
        let request_handle_path = screencast_proxy
            .create_session(ScreenCastOptions {
                //handle_token,
                session_handle_token: session_handle_token.clone(),
            })
            .await?;

        let path_str = request_handle_path.as_str();
        let path = path_str[0..path_str.len() - 1].to_string() + &session_handle_token;
        let request_handle_path = ObjectPath::try_from(path.clone())?;
        println!("request_handle_path {:?}", &request_handle_path);
        let session_handle = ObjectPath::try_from(path.replace("/request/", "/session/"))?;

        let request_proxy = RequestProxy::builder(&conn).path(request_handle_path)?.build().await?;

        let mut handle_blob = request_proxy.receive_response().await?;

        let _res = handle_blob.next();

        println!("session_handle {:?}", &session_handle);

        //>>>>>> SelectSource
        let request_handle_path = screencast_proxy
            .select_sources(
                session_handle.clone(),
                SelectSourcesOptions {
                    multiples: Some(false),
                    cursor_mode: Some(2),
                    types: Some(3),
                    restore_token,
                    persist_mode: Some(2),
                    ..Default::default()
                },
            )
            .await?;

        let request_proxy = RequestProxy::builder(&conn).path(request_handle_path)?.build().await?;

        let mut handle_blob = request_proxy.receive_response().await?;
        let _res = handle_blob.next();

        //>>>>>> Start
        let request_handle_path = screencast_proxy
            .start(session_handle.clone(), "".to_string(), StartOptions { ..Default::default() })
            .await?;

        let request_proxy = RequestProxy::builder(&conn).path(request_handle_path)?.build().await?;

        let mut handle_blob = request_proxy.receive_response().await?;

        let mut node_id = 0;

        let response = match handle_blob.next().await {
            Some(r) => Ok(r),
            None => Err(zbus::Error::Failure("Start doesn't have a response".to_string())),
        }?;

        let handle_blab = response.args()?;

        // Récupérer et sauvegarder le restore_token s'il est disponible
        if let Some(Value::Str(token)) = handle_blab.results().get("restore_token") {
            let token_str = token.as_str().to_string();
            println!("Received restore_token: {}", token_str);
            Self::save_restore_token(&token_str);
        }

        let streams = handle_blab.results().get("streams").unwrap();
        if let Value::Array(main_array) = streams {
            if let Some(sub_structure) = main_array.get::<Structure>(0)? {
                for struct_field in sub_structure.fields() {
                    if let Value::Dict(dict) = struct_field {
                        let key = &Str::from_static("id");
                        let id = match dict.get::<Str, Value>(key)? {
                            Some(value) => {
                                if let Value::Str(value_str) = value {
                                    value_str.as_str().to_string()
                                } else {
                                    value.to_string()
                                }
                            }
                            _ => "unknown".to_string(),
                        };
                        let key = &Str::from_static("size");
                        let size: String = match dict.get::<Str, Value>(key)? {
                            Some(value) => {
                                if let Value::Structure(structure) = value.try_clone()? {
                                    let fields = structure.into_fields();
                                    if fields.len() == 2 {
                                        format!(
                                            "{:?}x{:?}",
                                            i32_to_str(fields.first().unwrap()),
                                            i32_to_str(fields.last().unwrap())
                                        )
                                    } else {
                                        value.to_string()
                                    }
                                } else {
                                    value.to_string()
                                }
                            }
                            _ => "unknown".to_string(),
                        };
                        let key = &Str::from_static("position");
                        let position: String = match dict.get::<Str, Value>(key)? {
                            Some(value) => {
                                if let Value::Structure(structure) = value.try_clone()? {
                                    let fields = structure.into_fields();
                                    if fields.len() == 2 {
                                        format!(
                                            "(x:{:?} y:{:?})",
                                            i32_to_str(fields.first().unwrap()),
                                            i32_to_str(fields.last().unwrap())
                                        )
                                    } else {
                                        value.to_string()
                                    }
                                } else {
                                    value.to_string()
                                }
                            }
                            _ => "(x:0 y:0)".to_string(),
                        };
                        let key = &Str::from_static("source_type");
                        let source_type = match dict.get::<Str, Value>(key)? {
                            Some(value) => {
                                if let Value::U32(value_str) = value {
                                    value_str.to_string()
                                } else {
                                    value.to_string()
                                }
                            }
                            _ => "unknown".to_string(),
                        };
                        let key = &Str::from_static("mapping_id");
                        let mapping_id = match dict.get::<Str, Value>(key)? {
                            Some(value) => {
                                if let Value::Str(value_str) = value {
                                    value_str.as_str().to_string()
                                } else {
                                    value.to_string()
                                }
                            }
                            _ => "unknown".to_string(),
                        };
                        println!(
                            "ScreenCast {} launched with size {} at {} / Source : {} / Mapping Id : {}",
                            id, size, position, source_type, mapping_id
                        );
                    } else if let Value::U32(value) = struct_field {
                        node_id = *value;
                    }
                }
            }
        }
        //>>>>> OpenPipeWireRemote

        let msg = conn
            .call_method(
                Some("org.freedesktop.portal.Desktop"),
                "/org/freedesktop/portal/desktop",
                Some("org.freedesktop.portal.ScreenCast"),
                "OpenPipeWireRemote",
                &(session_handle, NoOptions {}),
            )
            .await?
            .body();

        let file_d: Fd = msg.deserialize()?;

        // Dupliquer le fd pour qu'il reste valide
        use std::os::fd::AsFd;
        let raw_fd = file_d.as_raw_fd();
        let owned_fd = file_d.as_fd().try_clone_to_owned()?;
        println!("PipeWire fd={} (duped={}) node={}", raw_fd, owned_fd.as_raw_fd(), node_id);

        let mut gst_str = format!(
            "pipewiresrc fd={} path={} ! queue ! videoconvert ! video/x-raw,format=I420 ! queue ",
            owned_fd.as_raw_fd(),
            node_id
        );

        encoding_str(coding, encoder, &mut gst_str);

        // Garder le fd ouvert
        Ok(Self {
            _desktop: Desktop::Wayland,
            connexion: gst_str,
            to_ip,
            _id: node_id,
            _screencast_proxy: Some(screencast_proxy),
            _conn: Some(conn),
            _pw_fd: Some(owned_fd),
        })
    }

    async fn main_loop(&mut self) -> Result<(), anyhow::Error> {
        let get_element_name = |msg: &MessageRef| match msg.src() {
            Some(element) => element.name(),
            None => "<No Element>".into(),
        };
        let connexion = &format!("{} ! queue ! udpsink {} ", &self.connexion, &self.to_ip);
        println!("connexion : {}", connexion);
        let pipeline = gstreamer::parse::launch(connexion)?;

        let pipelin_ = pipeline.dynamic_cast::<gstreamer::Bin>().unwrap();

        if let Some(bus) = pipelin_.bus() {
            // pipeline.set_state(gstreamer::State::Ready)?;
            println!("Ready !");
            pipelin_.set_state(gstreamer::State::Playing)?;
            println!("Playing !");

            for msg in bus.iter_timed(gstreamer::ClockTime::NONE) {
                use gstreamer::MessageView;

                match msg.view() {
                    MessageView::Eos(..) => {
                        println!("EOS  !");
                        if let Err(err) = pipelin_.set_state(gstreamer::State::Ready) {
                            println!("Error when closing pipeline : {:?}", err);
                        }
                        if let Err(err) = pipelin_.set_state(gstreamer::State::Null) {
                            println!("Error when closing pipeline : {:?}", err);
                        }
                        break;
                    }
                    MessageView::Error(err) => {
                        println!(
                            "Error from {:?}: {} ({:?})",
                            err.src().map(|s| s.path_string()),
                            err.error(),
                            err.debug()
                        );
                        if let Err(err) = pipelin_.set_state(gstreamer::State::Ready) {
                            println!("Error when closing pipeline : {:?}", err);
                        }
                        if let Err(err) = pipelin_.set_state(gstreamer::State::Null) {
                            println!("Error when closing pipeline : {:?}", err);
                        }
                        bail!(err.error());
                    }
                    MessageView::Warning(warning) => {
                        println!(
                            "Warning from {:?}: {} ({:?})",
                            warning.src().map(|s| s.path_string()),
                            warning.error(),
                            warning.debug()
                        );
                    }
                    MessageView::Info(info) => {
                        let element = get_element_name(info.message());
                        println!("Info on {:?} -> {:?}", element, info.message());
                    }
                    MessageView::StateChanged(s) => {
                        let element = get_element_name(s.message());
                        println!("{:?} on {} !", s.current(), element);
                    }
                    MessageView::StreamCollection(coll) => {
                        let element = get_element_name(coll.message());
                        let collection = coll.stream_collection();

                        println!("Collection from {:?} on {} !", collection.name(), element);
                    }
                    MessageView::StreamStart(s) => {
                        let element = get_element_name(s.message());
                        println!("StreamStart on {} !", element);
                    }
                    MessageView::Latency(_l) => {
                        let err = pipelin_.recalculate_latency();
                        println!("Latency  -> {:?} !", err);
                    }
                    _ => (),
                }
            }

            pipelin_.set_state(gstreamer::State::Null)?;
            Ok(())
        } else {
            // unlikely
            bail!("Unable to get a bus");
        }
    }

    async fn main_loop_server(&mut self) -> Result<(), anyhow::Error> {
        let main_loop = MainLoop::new(None, false);
        let server = gstreamer_rtsp_server::RTSPServer::new();
        let mounts = server.mount_points().expect("mount_points unavailable");
        let factory = gstreamer_rtsp_server::RTSPMediaFactory::new();
        factory.set_launch(self.connexion.clone().as_str());
        factory.set_shared(true);
        mounts.add_factory("/test", factory);
        let id = server.attach(None)?;
        println!("Stream ready at rtsp://127.0.0.1:{}/test", server.bound_port());

        // Start the mainloop. From this point on, the server will start to serve
        // our quality content to connecting clients.
        main_loop.run();
        println!("Server stopped!");
        id.remove();
        Ok(())
    }
}

pub fn i32_to_str(value: &Value<'_>) -> String {
    if let Value::I32(val) = value {
        format!("{}", val)
    } else {
        format!("{:?}", value)
    }
}

#[derive(Debug, Clone, PartialEq, Default, Type, DeserializeDict, SerializeDict)]
#[zvariant(signature = "a{sv}")]
pub struct ScreenCastOptions {
    //pub handle_token: String,
    pub session_handle_token: String,
}

#[derive(Debug, Clone, PartialEq, Default, Type, DeserializeDict, SerializeDict)]
#[zvariant(signature = "a{sv}")]
pub struct SelectSourcesOptions {
    pub handle_token: Option<String>,
    pub types: Option<u32>,
    pub multiples: Option<bool>,
    pub cursor_mode: Option<u32>,
    pub restore_token: Option<String>,
    pub persist_mode: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Default, Type, DeserializeDict, SerializeDict)]
#[zvariant(signature = "a{sv}")]
pub struct StartOptions {
    pub handle_token: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Default, Type, DeserializeDict, SerializeDict)]
#[zvariant(signature = "a{sv}")]
pub struct NoOptions {}

#[proxy(
    interface = "org.freedesktop.portal.ScreenCast",
    default_service = "org.freedesktop.portal.Desktop",
    default_path = "/org/freedesktop/portal/desktop"
)]
trait ScreenCast {
    #[zbus(property)]
    fn available_source_types(&self) -> zbus::Result<u32>;
    #[zbus(property)]
    fn available_cursor_modes(&self) -> zbus::Result<u32>;
    #[zbus(property)]
    fn version(&self) -> zbus::Result<u32>;
    // fn
    fn create_session(&self, options: ScreenCastOptions) -> zbus::Result<OwnedObjectPath>;
    fn select_sources(
        &self,
        session_handle: ObjectPath<'_>,
        options: SelectSourcesOptions,
    ) -> zbus::Result<OwnedObjectPath>;
    fn start(
        &self,
        session_handle: ObjectPath<'_>,
        parent_window: String,
        options: StartOptions,
    ) -> zbus::Result<OwnedObjectPath>;
    // fn open_pipe_wire_remote(
    //     &self,
    //     session_handle: ObjectPath<'_>,
    //     options: NoOptions,
    // ) -> zbus::Result<OwnedFd>;
}

#[proxy(
    interface = "org.freedesktop.portal.Session",
    default_service = "org.freedesktop.portal.Desktop",
    default_path = "/org/freedesktop/portal/desktop"
)]
trait Session {
    #[zbus(property)]
    fn version(&self) -> zbus::Result<u32>;
    // fn
    fn close(&self) -> zbus::Result<()>;
    #[zbus(signal)]
    fn closed(&self) -> zbus::Result<()>;
}

#[zbus::proxy(
    interface = "org.freedesktop.portal.Request",
    default_service = "org.freedesktop.portal.Desktop",
    default_path = "/org/freedesktop/portal/desktop"
)]
trait Request {
    #[zbus(signal)]
    fn response(&self, response: u32, results: HashMap<&str, zbus::zvariant::Value<'_>>) -> zbus::Result<()>;
    // fn
    fn close(&self) -> zbus::Result<()>;
}
