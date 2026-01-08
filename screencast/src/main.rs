use anyhow::{bail, Context, Result};
use ashpd::desktop::{
    screencast::{CursorMode, Screencast, SourceType},
    PersistMode,
};
use eframe::egui::{self};
use futures::stream::StreamExt;
use gstreamer::{glib::MainLoop, prelude::*};
use gstreamer_rtsp_server::prelude::{RTSPMediaFactoryExt, RTSPMountPointsExt, RTSPServerExt, RTSPServerExtManual};
use std::sync::atomic::{AtomicBool, Ordering};
// Global variable to transmit the restore session state
static RESTORE_SESSION: AtomicBool = AtomicBool::new(false);
use std::{fs, os::fd::AsRawFd, path::PathBuf, time::Instant};

pub const DEFAULT_HOST: &str = "host=192.168.1.131 port=5000";

fn main() -> Result<(), eframe::Error> {
    env_logger::init();
    let rt = tokio::runtime::Runtime::new().unwrap();
    let _guard = rt.enter();

    let _err = gstreamer::init();
    gstreamer::log::set_default_threshold(gstreamer::DebugLevel::Warning);

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([500.0, 600.0]),
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

#[derive(Debug, Clone)]
struct StreamInfo {
    ip: String,
    port: String,
    encoder: Option<String>,
    bitrate: Option<u32>,
    min_bitrate: Option<u32>,
    max_bitrate: Option<u32>,
    last_update: Instant,
}

impl Default for StreamInfo {
    fn default() -> Self {
        Self {
            ip: String::new(),
            port: String::new(),
            encoder: None,
            bitrate: None,
            min_bitrate: None,
            max_bitrate: None,
            last_update: Instant::now(),
        }
    }
}

struct MyApp {
    desktop: Desktop,
    window_id: String,
    streams: Vec<(
        tokio::task::JoinHandle<()>,
        tokio::sync::broadcast::Sender<()>,
        tokio::sync::mpsc::UnboundedReceiver<StreamInfo>,
    )>,
    stream_infos: Vec<StreamInfo>,
    coding: Coding,
    fps: u32,
    with_gpu: bool,
    with_cuda: bool,
    with_rtsp: bool,
    to_ip: String,
}

impl MyApp {
    fn new(desktop: Desktop) -> Self {
        if desktop == Desktop::Wayland {
            Stream::load_restore_token();
        }
        Self {
            desktop,
            window_id: String::new(),
            streams: vec![],
            stream_infos: vec![],
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

        let (tx, rx) = tokio::sync::broadcast::channel(1);
        let (info_tx, info_rx) = tokio::sync::mpsc::unbounded_channel();

        // Parse IP and port before moving to_ip
        let initial_info = if !with_rtsp {
            // Parse IP and port from to_ip
            if let Some(host_str) = to_ip.strip_prefix("host=") {
                let parts: Vec<&str> = host_str.split(" port=").collect();
                if parts.len() == 2 {
                    StreamInfo { ip: parts[0].to_string(), port: parts[1].to_string(), ..Default::default() }
                } else {
                    StreamInfo::default()
                }
            } else {
                StreamInfo::default()
            }
        } else {
            StreamInfo { ip: "127.0.0.1".to_string(), port: "rtsp".to_string(), ..Default::default() }
        };

        let handle = tokio::spawn(async move {
            match Stream::run(desktop, index, coding, encoder, to_ip, window_id, fps, rx, with_rtsp, info_tx).await {
                Ok(_) => {
                    // Loop execution finished
                }
                Err(e) => eprintln!("Stream error: {:?}", e),
            }
        });

        // You cannot join a tokio::JoinHandle in a sync context, but you can store it if needed
        // For the UI, just count the number of launched streams
        self.stream_infos.push(initial_info);
        self.streams.push((handle, tx, info_rx));
    }
}

impl eframe::App for MyApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Request continuous repaint for smooth color animation
        ctx.request_repaint();

        // Receive stream info updates
        for (i, (_, _, info_rx)) in self.streams.iter_mut().enumerate() {
            if let Ok(info) = info_rx.try_recv() {
                if i < self.stream_infos.len() {
                    // Update only encoder and bitrate fields, keep ip and port
                    self.stream_infos[i].encoder = info.encoder;
                    self.stream_infos[i].bitrate = info.bitrate;
                    self.stream_infos[i].min_bitrate = info.min_bitrate;
                    self.stream_infos[i].max_bitrate = info.max_bitrate;
                    self.stream_infos[i].last_update = Instant::now();
                }
            }
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::both().auto_shrink([false, false]).show(ui, |ui| {
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

                // --- Stream monitoring panel ---
                ui.group(|ui| {
                    ui.heading("Active Streams Monitor");
                    if self.streams.is_empty() {
                        ui.label("No active streams.");
                    } else {
                        let mut to_remove = None;

                        // Display streams in 2 columns
                        egui::Grid::new("streams_grid").num_columns(2).spacing([20.0, 10.0]).striped(false).show(
                            ui,
                            |ui| {
                                for (i, (handle, tx, _)) in self.streams.iter().enumerate() {
                                    let status = if handle.is_finished() { "Finished" } else { "Running" };

                                    // Determine if stream is active (received update in last 2 seconds)
                                    let elapsed_secs = if i < self.stream_infos.len() {
                                        self.stream_infos[i].last_update.elapsed().as_secs_f32()
                                    } else {
                                        10.0
                                    };

                                    // Interpolate color from green to red based on elapsed time
                                    // 0s = green (immediate), 10s+ = red (slow transition)
                                    let transition = (elapsed_secs / 10.0).clamp(0.0, 1.0);
                                    let green_intensity = ((1.0 - transition) * 80.0) as u8;
                                    let red_intensity = (40.0 + transition * 40.0) as u8;
                                    let bg_color = egui::Color32::from_rgb(red_intensity, green_intensity, 40);

                                    egui::Frame::new().fill(bg_color).inner_margin(12.0).corner_radius(4.0).show(
                                        ui,
                                        |ui| {
                                            ui.vertical(|ui| {
                                                ui.set_min_width(220.0);
                                                ui.horizontal(|ui| {
                                                    ui.strong(format!("Stream #{}", i + 1));
                                                    ui.label(format!("- {}", status));
                                                    if ui.button("🛑").clicked() {
                                                        to_remove = Some(i);
                                                        let _ = tx.send(());
                                                    }
                                                });

                                                // Display stream info
                                                if i < self.stream_infos.len() {
                                                    let info = &self.stream_infos[i];
                                                    ui.label(format!("🌐 {}:{}", info.ip, info.port));
                                                    if let Some(encoder) = &info.encoder {
                                                        ui.label(format!("🔧 Encoder: {}", encoder));
                                                    }
                                                    if let Some(bitrate) = info.bitrate {
                                                        ui.label(format!("📊 Bitrate: {} kbps", bitrate / 1000));
                                                    }
                                                    if let Some(min) = info.min_bitrate {
                                                        if let Some(max) = info.max_bitrate {
                                                            ui.label(format!(
                                                                "📊 Range: {} - {} kbps",
                                                                min / 1000,
                                                                max / 1000
                                                            ));
                                                        }
                                                    }
                                                }
                                            });
                                        },
                                    );

                                    // After every 2 items, start a new row
                                    if (i + 1) % 2 == 0 {
                                        ui.end_row();
                                    }
                                }
                            },
                        );

                        // Remove the selected stream handle (actual cancellation needs Stream-side support)
                        if let Some(idx) = to_remove {
                            self.streams.remove(idx);
                            if idx < self.stream_infos.len() {
                                self.stream_infos.remove(idx);
                            }
                        }
                    }
                });
                ui.separator();
                let restore_session = RESTORE_SESSION.load(Ordering::Relaxed);
                ui.horizontal(|ui| {
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
                    if restore_session && ui.button("🔄 Reset & Start ScreenCast").clicked() {
                        // Reset RESTORE_SESSION (and thus the restore_token)
                        Stream::save_restore_token("");
                        let encoder = if self.with_cuda {
                            Encoder::NvidiaCuda
                        } else if self.with_gpu {
                            Encoder::Nvidia
                        } else {
                            Encoder::Raw
                        };
                        self.launch_stream(self.coding, encoder);
                    }
                });

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
        });
    }
}

struct Stream;

impl Stream {
    fn config_path() -> PathBuf {
        // Get the config path for saving the restore token
        let mut path = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
        path.push("screencast");
        fs::create_dir_all(&path).ok();
        path.push("session_config.txt");
        path
    }

    fn load_restore_token() -> Option<String> {
        // Load the restore token from the config file
        let path = Self::config_path();
        let result = fs::read_to_string(path).ok().and_then(|s| {
            for line in s.lines() {
                if let Some(token) = line.strip_prefix("restore_token = ") {
                    let token = token.trim().to_string();
                    if !token.is_empty() {
                        return Some(token);
                    }
                }
            }
            None
        });
        // Update RESTORE_SESSION according to the presence of the token
        RESTORE_SESSION.store(result.is_some(), std::sync::atomic::Ordering::Relaxed);
        result
    }

    fn save_restore_token(token: &str) {
        // Save the restore token to the config file
        let path = Self::config_path();
        let line = format!("restore_token = {}", token);
        if let Err(e) = fs::write(&path, line) {
            eprintln!("Failed to save restore_token to {:?}: {}", path, e);
        } else {
            println!("Saved restore_token to {:?}", path);
        }
        // Update RESTORE_SESSION according to the presence of the token
        RESTORE_SESSION.store(!token.trim().is_empty(), std::sync::atomic::Ordering::Relaxed);
    }

    #[allow(clippy::too_many_arguments)]
    async fn run(
        desktop: Desktop,
        _index: String,
        coding: Coding,
        encoder: Encoder,
        to_ip: String,
        window_id: String,
        fps: u32,
        stop_signal: tokio::sync::broadcast::Receiver<()>,
        with_rtsp: bool,
        info_tx: tokio::sync::mpsc::UnboundedSender<StreamInfo>,
    ) -> Result<()> {
        match desktop {
            Desktop::Wayland => Self::run_wayland(coding, encoder, to_ip, stop_signal, with_rtsp, info_tx).await,
            Desktop::X11 => {
                Self::run_x11(coding, encoder, to_ip, window_id, fps, stop_signal, with_rtsp, info_tx).await
            }
            Desktop::Windows => bail!("Windows not yet supported"),
            Desktop::MacOs => bail!("MacOS not yet supported"),
        }
    }

    async fn run_wayland(
        coding: Coding,
        encoder: Encoder,
        to_ip: String,
        stop_signal: tokio::sync::broadcast::Receiver<()>,
        with_rtsp: bool,
        info_tx: tokio::sync::mpsc::UnboundedSender<StreamInfo>,
    ) -> Result<()> {
        println!("🎥 Starting Wayland screencast using ashpd...");

        let proxy = Screencast::new().await?;

        // Load restore_token if available
        let token = Self::load_restore_token();
        if let Some(ref t) = token {
            println!("📋 Using saved restore_token: {}", t);
        } else {
            println!("📋 No restore_token found, will request new session");
        }

        // Create screencast session
        let session = proxy.create_session().await.context("Failed to create screencast session")?;

        println!("✓ Session created");

        // Select sources (monitor + windows)
        proxy
            .select_sources(
                &session,
                CursorMode::Embedded,                     // Include cursor in capture
                SourceType::Monitor | SourceType::Window, // Capture monitors and windows
                false,                                    // multiple: false (single source)
                token.as_deref(),
                PersistMode::ExplicitlyRevoked, // Save until explicitly revoked
            )
            .await
            .context("Failed to select sources")?;

        println!("✓ Sources selected");

        // Start capture
        let response = proxy.start(&session, None).await.context("Failed to start screencast")?;

        println!("✓ Screencast started");

        // Get response data
        let streams_data = response.response()?;

        // Save restore_token if available, otherwise clear previous value
        if let Some(token) = streams_data.restore_token() {
            println!("💾 Received new restore_token");
            Self::save_restore_token(token);
        } else {
            // Clear the restore_token file with an empty value
            println!("💾 No restore_token received, clearing saved token");
            Self::save_restore_token("");
        }

        // Get streams
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

        // Open the PipeWire remote
        let fd = proxy.open_pipe_wire_remote(&session).await.context("Failed to open PipeWire remote")?;

        let fd_num = fd.as_raw_fd();
        println!("🔌 PipeWire fd: {}", fd_num);

        // Build the GStreamer pipeline
        let mut pipeline_str = format!(
            "pipewiresrc fd={} path={} ! queue ! videoconvert ! video/x-raw,format=I420 ! queue ",
            fd_num, node_id
        );

        append_encoding(&mut pipeline_str, coding, encoder);

        let result = if with_rtsp {
            Self::main_loop_server(pipeline_str, stop_signal).await
        } else {
            Self::main_loop(pipeline_str, to_ip, stop_signal, info_tx).await
        };

        println!("🚪 Closing session explicitly (Wayland)");
        let _ = session.close().await;

        // Ensure fd is dropped
        drop(fd);

        result
    }

    #[allow(clippy::too_many_arguments)]
    async fn run_x11(
        coding: Coding,
        encoder: Encoder,
        to_ip: String,
        window_id: String,
        fps: u32,
        stop_signal: tokio::sync::broadcast::Receiver<()>,
        with_rtsp: bool,
        info_tx: tokio::sync::mpsc::UnboundedSender<StreamInfo>,
    ) -> Result<()> {
        println!("🎥 Starting X11 screencast using ximagesrc...");

        let xid = if window_id.is_empty() { String::new() } else { format!("endx={} endy={}", window_id, window_id) };

        let mut pipeline_str = format!(
            "ximagesrc {} use-damage=false remote=1 blocksize=16384 ! video/x-raw, framerate={}/1 ! queue ",
            xid, fps
        );

        append_encoding(&mut pipeline_str, coding, encoder);

        if with_rtsp {
            Self::main_loop_server(pipeline_str, stop_signal).await
        } else {
            Self::main_loop(pipeline_str, to_ip, stop_signal, info_tx).await
        }
    }

    async fn main_loop(
        pipeline_str: String,
        to_ip: String,
        mut stop_signal: tokio::sync::broadcast::Receiver<()>,
        info_tx: tokio::sync::mpsc::UnboundedSender<StreamInfo>,
    ) -> Result<()> {
        let connection_str = format!("{} ! queue ! udpsink {} ", pipeline_str, to_ip);
        println!("🔗 Pipeline: {}", connection_str);

        let pipeline = gstreamer::parse::launch(&connection_str)?;
        let pipeline = pipeline.dynamic_cast::<gstreamer::Bin>().unwrap();

        if let Some(bus) = pipeline.bus() {
            pipeline.set_state(gstreamer::State::Playing)?;
            println!("▶️  Playing!");

            let mut bus_stream = bus.stream();

            loop {
                tokio::select! {
                    _ = stop_signal.recv() => {
                        println!("🛑 Stop signal received");
                        break;
                    }
                    msg = bus_stream.next() => {
                        match msg {
                            Some(msg) => {
                                use gstreamer::MessageView;

                                match msg.view() {
                                    MessageView::Eos(..) => {
                                        println!("⏹️  End of stream");
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
                                    MessageView::Tag(tag) => {
                                        let tags = tag.tags();

                                        // Extract tag information
                                        let mut stream_info = StreamInfo::default();

                                        if let Some(encoder) = tags.get::<gstreamer::tags::Encoder>() {
                                            stream_info.encoder = Some(encoder.get().to_string());
                                        }

                                        if let Some(bitrate) = tags.get::<gstreamer::tags::Bitrate>() {
                                            stream_info.bitrate = Some(bitrate.get());
                                        }

                                        if let Some(min_bitrate) = tags.get::<gstreamer::tags::MinimumBitrate>() {
                                            stream_info.min_bitrate = Some(min_bitrate.get());
                                        }

                                        if let Some(max_bitrate) = tags.get::<gstreamer::tags::MaximumBitrate>() {
                                            stream_info.max_bitrate = Some(max_bitrate.get());
                                        }

                                        // Send info to UI (ignore if receiver is closed)
                                        let _ = info_tx.send(stream_info);
                                    }
                                    _otherwise => {
                                        //println!("ℹ️  Other message: {:?}", otherwise);
                                    }
                                }
                            }
                            None => {
                                println!("🚫 Bus stream ended unexpectedly");
                                break;
                            }
                        }
                    }
                }
            }

            println!("🛑 Stopping pipeline...");
            pipeline.set_state(gstreamer::State::Null)?;
            Ok(())
        } else {
            bail!("Unable to get bus from pipeline")
        }
    }

    async fn main_loop_server(
        pipeline_str: String,
        mut stop_signal: tokio::sync::broadcast::Receiver<()>,
    ) -> Result<()> {
        println!("🌐 Starting RTSP server...");

        let main_loop = MainLoop::new(None, false);
        let server = gstreamer_rtsp_server::RTSPServer::new();
        let mounts = server.mount_points().expect("mount_points unavailable");
        let factory = gstreamer_rtsp_server::RTSPMediaFactory::new();

        factory.set_launch(&pipeline_str);
        factory.set_shared(true);
        mounts.add_factory("/test", factory);

        let id = server.attach(None)?;
        println!("📡 RTSP server ready at rtsp://127.0.0.1:{}/test", server.bound_port());

        let loop_clone = main_loop.clone();
        // Spawn thread for the blocking loop
        std::thread::spawn(move || {
            loop_clone.run();
        });

        // Wait for stop signal
        let _ = stop_signal.recv().await;

        println!("🛑 Stop signal received for RTSP server");
        main_loop.quit();

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
