use anyhow::{bail, Context, Result};
use ashpd::desktop::{
    screencast::{CursorMode, Screencast, SourceType},
    PersistMode,
};
use eframe::egui::{self};
use gstreamer::{glib::MainLoop, prelude::*};
use gstreamer_rtsp_server::prelude::{RTSPMediaFactoryExt, RTSPMountPointsExt, RTSPServerExt, RTSPServerExtManual};
use std::{fs, os::fd::AsRawFd, path::PathBuf};

pub const DEFAULT_HOST: &str = "host=192.168.1.131 port=5000";

fn main() -> Result<(), eframe::Error> {
    env_logger::init();
    let rt = tokio::runtime::Runtime::new().unwrap();
    let _guard = rt.enter();

    let _err = gstreamer::init();
    gstreamer::log::set_default_threshold(gstreamer::DebugLevel::Warning);

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([400.0, 300.0]),
        ..Default::default()
    };

    let desktop = detect_desktop();

    eframe::run_native(
        "XR ScreenCast (ashpd)",
        options,
        Box::new(|cc| {
            egui_extras::install_image_loaders(&cc.egui_ctx);
            Ok(Box::new(MyApp::new(desktop)))
        }),
    )
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Desktop {
    Wayland,
    X11,
    Windows,
    MacOs,
}

fn detect_desktop() -> Desktop {
    if cfg!(target_os = "linux") {
        if std::env::var("WAYLAND_DISPLAY").is_ok() {
            Desktop::Wayland
        } else {
            Desktop::X11
        }
    } else if cfg!(target_os = "windows") {
        Desktop::Windows
    } else {
        Desktop::MacOs
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Coding {
    H264,
    H265,
    VP9,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Encoder {
    Nvidia,
    NvidiaCuda,
    Raw,
}

struct MyApp {
    desktop: Desktop,
    window_id: String,
    streams: Vec<tokio::task::JoinHandle<()>>,
    coding: Coding,
    fps: u32,
    with_gpu: bool,
    with_cuda: bool,
    with_rtsp: bool,
    to_ip: String,
}

impl MyApp {
    fn new(desktop: Desktop) -> Self {
        Self {
            desktop,
            window_id: String::new(),
            streams: vec![],
            coding: Coding::H264,
            fps: 60,
            with_gpu: true,
            with_cuda: true,
            with_rtsp: false,
            to_ip: DEFAULT_HOST.into(),
        }
    }

    fn launch_stream(&mut self, coding: Coding, encoder: Encoder) {
        let index = (self.streams.len() + 1).to_string();
        let desktop = self.desktop;
        let with_rtsp = self.with_rtsp;
        let to_ip = self.to_ip.clone();
        let window_id = self.window_id.clone();
        let fps = self.fps;

        let handle = tokio::spawn(async move {
            match Stream::new(desktop, index, coding, encoder, to_ip, window_id, fps).await {
                Ok(mut stream) => {
                    let result = if with_rtsp { stream.main_loop_server().await } else { stream.main_loop().await };
                    if let Err(e) = result {
                        eprintln!("Stream error: {:?}", e);
                    }
                }
                Err(e) => eprintln!("Failed to create stream: {:?}", e),
            }
        });

        // On ne peut pas join un tokio::JoinHandle dans un contexte sync, mais on peut le stocker si besoin
        // Pour l'UI, on peut juste compter le nombre de streams lancés
        self.streams.push(handle);
    }
}

impl eframe::App for MyApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("XR ScreenCast (ashpd)");

            ui.horizontal(|ui| {
                let window_id_label = ui.label("Window ID (X11):");
                ui.text_edit_singleline(&mut self.window_id).labelled_by(window_id_label.id);
            });

            ui.horizontal(|ui| {
                let to_ip = ui.label("Cast to:");
                ui.text_edit_singleline(&mut self.to_ip).labelled_by(to_ip.id);
            });

            ui.separator();

            ui.horizontal(|ui| {
                ui.radio_value(&mut self.coding, Coding::H264, "H264");
                ui.radio_value(&mut self.coding, Coding::H265, "H265");
                ui.radio_value(&mut self.coding, Coding::VP9, "VP9");
            });

            ui.horizontal(|ui| {
                ui.checkbox(&mut self.with_gpu, "GPU (NVIDIA)");
                ui.checkbox(&mut self.with_cuda, "CUDA");
                ui.checkbox(&mut self.with_rtsp, "RTSP");
            });

            if self.with_cuda {
                self.with_gpu = true;
            }
            if !self.with_gpu {
                self.with_cuda = false;
            }

            ui.add(egui::Slider::new(&mut self.fps, 15..=120).text("FPS"));

            ui.separator();

            if ui.button(format!("🎬 Start ScreenCast ({:?})", self.coding)).clicked() {
                let encoder = if self.with_cuda {
                    Encoder::NvidiaCuda
                } else if self.with_gpu {
                    Encoder::Nvidia
                } else {
                    Encoder::Raw
                };
                self.launch_stream(self.coding, encoder);
            }

            ui.label(format!("Active streams: {}", self.streams.len()));

            if self.desktop == Desktop::Wayland {
                ui.separator();
                ui.colored_label(egui::Color32::GREEN, "✓ Wayland detected - using ashpd portal");
            } else if self.desktop == Desktop::X11 {
                ui.colored_label(egui::Color32::YELLOW, "⚠ X11 detected - using ximagesrc");
            }

            ui.separator();
            ui.image(egui::include_image!("../../res/mipmap-hdpi/app_icon.png"));
        });
    }
}

struct Stream {
    pipeline_str: String,
    to_ip: String,
    _node_id: Option<u32>,
}

impl Stream {
    fn config_path() -> PathBuf {
        let mut path = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
        path.push("screencast");
        fs::create_dir_all(&path).ok();
        path.push("session_config.txt");
        path
    }

    fn load_restore_token() -> Option<String> {
        let path = Self::config_path();
        fs::read_to_string(path).ok().and_then(|s| {
            let token = s.trim().to_string();
            if !token.is_empty() {
                Some(token)
            } else {
                None
            }
        })
    }

    fn save_restore_token(token: &str) {
        let path = Self::config_path();
        if let Err(e) = fs::write(&path, token) {
            eprintln!("Failed to save restore_token to {:?}: {}", path, e);
        } else {
            println!("Saved restore_token to {:?}", path);
        }
    }

    async fn new(
        desktop: Desktop,
        _index: String,
        coding: Coding,
        encoder: Encoder,
        to_ip: String,
        window_id: String,
        fps: u32,
    ) -> Result<Self> {
        match desktop {
            Desktop::Wayland => Self::new_wayland(coding, encoder, to_ip).await,
            Desktop::X11 => Self::new_x11(coding, encoder, to_ip, window_id, fps).await,
            Desktop::Windows => bail!("Windows not yet supported"),
            Desktop::MacOs => bail!("MacOS not yet supported"),
        }
    }

    async fn new_wayland(coding: Coding, encoder: Encoder, to_ip: String) -> Result<Self> {
        println!("🎥 Starting Wayland screencast using ashpd...");

        let proxy = Screencast::new().await?;

        // Charger le restore_token s'il existe
        let restore_token = Self::load_restore_token();
        if let Some(ref token) = restore_token {
            println!("📋 Using saved restore_token: {}", token);
        } else {
            println!("📋 No restore_token found, will request new session");
        }

        // Créer une session de screencast
        let session = proxy.create_session().await.context("Failed to create screencast session")?;

        println!("✓ Session created");

        // Sélectionner les sources (moniteur + fenêtres)
        proxy
            .select_sources(
                &session,
                CursorMode::Embedded,                     // Inclure le curseur dans la capture
                SourceType::Monitor | SourceType::Window, // Capturer moniteurs et fenêtres
                false,                                    // multiple: false (une seule source)
                restore_token.as_deref(),
                PersistMode::ExplicitlyRevoked, // Sauvegarder jusqu'à révocation explicite
            )
            .await
            .context("Failed to select sources")?;

        println!("✓ Sources selected");

        // Démarrer la capture
        let response = proxy.start(&session, None).await.context("Failed to start screencast")?;

        println!("✓ Screencast started");

        // Récupérer les données de la réponse
        let streams_data = response.response()?;

        // Sauvegarder le restore_token si disponible
        if let Some(token) = streams_data.restore_token() {
            println!("💾 Received new restore_token");
            Self::save_restore_token(token);
        }

        // Récupérer les streams
        let streams = streams_data.streams();
        if streams.is_empty() {
            bail!("No streams available");
        }

        let stream = &streams[0];
        let node_id = stream.pipe_wire_node_id();

        println!("📺 Stream info:");
        println!("  - Node ID: {}", node_id);
        if let Some((width, height)) = stream.size() {
            println!("  - Size: {}x{}", width, height);
        }
        if let Some((x, y)) = stream.position() {
            println!("  - Position: ({}, {})", x, y);
        }

        // Ouvrir le PipeWire remote
        let fd = proxy.open_pipe_wire_remote(&session).await.context("Failed to open PipeWire remote")?;

        let fd_num = fd.as_raw_fd();
        println!("🔌 PipeWire fd: {}", fd_num);

        // Construire la pipeline GStreamer
        let mut gst_str = format!(
            "pipewiresrc fd={} path={} ! queue ! videoconvert ! video/x-raw,format=I420 ! queue ",
            fd_num, node_id
        );

        append_encoding(&mut gst_str, coding, encoder);

        // Garder le fd en vie en le "leakant" (ne pas le fermer)
        std::mem::forget(fd);

        Ok(Self { pipeline_str: gst_str, to_ip, _node_id: Some(node_id) })
    }

    async fn new_x11(coding: Coding, encoder: Encoder, to_ip: String, window_id: String, fps: u32) -> Result<Self> {
        println!("🎥 Starting X11 screencast using ximagesrc...");

        let xid = if window_id.is_empty() { String::new() } else { format!("endx={} endy={}", window_id, window_id) };

        let mut gst_str = format!(
            "ximagesrc {} use-damage=false remote=1 blocksize=16384 ! video/x-raw, framerate={}/1 ! queue ",
            xid, fps
        );

        append_encoding(&mut gst_str, coding, encoder);

        Ok(Self { pipeline_str: gst_str, to_ip, _node_id: None })
    }

    async fn main_loop(&mut self) -> Result<()> {
        let connection_str = format!("{} ! queue ! udpsink {} ", &self.pipeline_str, &self.to_ip);
        println!("🔗 Pipeline: {}", connection_str);

        let pipeline = gstreamer::parse::launch(&connection_str)?;
        let pipeline = pipeline.dynamic_cast::<gstreamer::Bin>().unwrap();

        if let Some(bus) = pipeline.bus() {
            pipeline.set_state(gstreamer::State::Playing)?;
            println!("▶️  Playing!");

            for msg in bus.iter_timed(gstreamer::ClockTime::NONE) {
                use gstreamer::MessageView;

                match msg.view() {
                    MessageView::Eos(..) => {
                        println!("⏹️  End of stream");
                        pipeline.set_state(gstreamer::State::Null)?;
                        break;
                    }
                    MessageView::Error(err) => {
                        eprintln!(
                            "❌ Error from {:?}: {} ({:?})",
                            err.src().map(|s| s.path_string()),
                            err.error(),
                            err.debug()
                        );
                        pipeline.set_state(gstreamer::State::Null)?;
                        bail!(err.error());
                    }
                    MessageView::Warning(warning) => {
                        println!("⚠️  Warning from {:?}: {}", warning.src().map(|s| s.path_string()), warning.error());
                    }
                    MessageView::StateChanged(s) => {
                        if let Some(src) = msg.src() {
                            if src == pipeline.upcast_ref::<gstreamer::Object>() {
                                println!("🔄 Pipeline state: {:?} -> {:?}", s.old(), s.current());
                            }
                        }
                    }
                    MessageView::Latency(_) => {
                        let _ = pipeline.recalculate_latency();
                    }
                    _ => (),
                }
            }

            pipeline.set_state(gstreamer::State::Null)?;
            Ok(())
        } else {
            bail!("Unable to get bus from pipeline")
        }
    }

    async fn main_loop_server(&mut self) -> Result<()> {
        println!("🌐 Starting RTSP server...");

        let main_loop = MainLoop::new(None, false);
        let server = gstreamer_rtsp_server::RTSPServer::new();
        let mounts = server.mount_points().expect("mount_points unavailable");
        let factory = gstreamer_rtsp_server::RTSPMediaFactory::new();

        factory.set_launch(&self.pipeline_str);
        factory.set_shared(true);
        mounts.add_factory("/test", factory);

        let id = server.attach(None)?;
        println!("📡 RTSP server ready at rtsp://127.0.0.1:{}/test", server.bound_port());

        main_loop.run();

        println!("⏹️  Server stopped");
        id.remove();
        Ok(())
    }
}

fn append_encoding(gst_str: &mut String, coding: Coding, encoder: Encoder) {
    match coding {
        Coding::H264 => match encoder {
            Encoder::Nvidia => {
                gst_str.push_str(
                    "! videoconvert ! nvautogpuh264enc bitrate=8192 gop-size=30 repeat-sequence-header=true aud=true \
                    ! h264parse config-interval=-1 \
                    ! video/x-h264,stream-format=byte-stream,alignment=au \
                    ! rtph264pay pt=96 config-interval=1 mtu=1200 ",
                );
            }
            Encoder::NvidiaCuda => {
                gst_str.push_str(
                    "! videoconvert ! nvh264enc bitrate=8192 rc-mode=vbr qp-const-b=0 qp-const-i=0 qp-const-p=0 \
                    gop-size=475 qos=true preset=low-latency-hq \
                    ! h264parse config-interval=-1 \
                    ! video/x-h264,stream-format=byte-stream,alignment=au,profile=baseline \
                    ! rtph264pay name=pay0 pt=96 config-interval=1 mtu=1200",
                );
            }
            Encoder::Raw => {
                gst_str.push_str(
                    "! videoconvert ! video/x-raw,format=I420 ! x264enc bitrate=8192 speed-preset=superfast \
                    tune=zerolatency byte-stream=true sliced-threads=true ! h264parse config-interval=-1 \
                    ! video/x-h264,stream-format=byte-stream ! rtph264pay",
                );
            }
        },
        Coding::H265 => match encoder {
            Encoder::Nvidia => {
                gst_str.push_str(
                    "! videoconvert ! nvautogpuh265enc bitrate=8192 gop-size=30 repeat-sequence-header=true aud=true \
                    ! h265parse config-interval=-1 \
                    ! video/x-h265,stream-format=byte-stream,alignment=au \
                    ! rtph265pay pt=96 config-interval=1 mtu=1200 ",
                );
            }
            Encoder::NvidiaCuda => {
                gst_str.push_str(
                    "! videoconvert ! nvh265enc bitrate=8192 rc-mode=vbr qp-const-b=0 qp-const-i=0 qp-const-p=0 \
                    gop-size=475 qos=true preset=low-latency-hq \
                    ! h265parse config-interval=-1 \
                    ! video/x-h265,stream-format=byte-stream,alignment=au \
                    ! rtph265pay name=pay0 pt=96 config-interval=1 mtu=1200",
                );
            }
            Encoder::Raw => {
                gst_str.push_str("! videoconvert ! x265enc speed-preset=superfast tune=zerolatency ! rtph265pay");
            }
        },
        Coding::VP9 => {
            gst_str.push_str("! videoconvert ! vp9enc ! rtpvp9pay");
        }
    }
}
