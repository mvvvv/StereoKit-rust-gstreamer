# Screencast

Screen capture application (screencast) using **ashpd** (XDG Desktop Portal) for Wayland and ximagesrc for X11.

## Features

- ✅ Wayland support via **ashpd** (XDG portal)
- ✅ X11 support via ximagesrc
- ✅ Session persistence (restore_token)
- ✅ H264, H265, VP9 encoding
- ✅ NVIDIA GPU and CUDA support
- ✅ RTSP or UDP mode
- ✅ Simple GUI with egui

## Advantages over screencast-vr

1. **Simplicity**: Uses `ashpd` which handles all D-Bus complexity
2. **Less code**: ~400 lines vs ~760 lines
3. **Type-safe**: ashpd provides idiomatic Rust types
4. **Maintenance**: ashpd is actively maintained and follows portal updates

## Build

```bash
cargo build -p screencast
```

## Run

```bash
# With detailed logs
RUST_LOG=debug cargo run -p screencast

# Without logs
cargo run -p screencast
```

## Configuration

The restore_token is saved in:

- Linux: `~/.config/screencast/session_config.txt`
- macOS: `~/Library/Application Support/screencast/session_config.txt`
- Windows: `%APPDATA%\screencast\session_config.txt`

## Usage

1. Select encoding (H264, H265, VP9)
2. Choose GPU/CUDA according to your hardware
3. Set the destination address
4. Click "Start ScreenCast"

## Dependencies

- `ashpd`: Rust interface for XDG Desktop Portals
- `eframe/egui`: GUI
- `gstreamer`: Streaming pipeline
- `tokio`: Async runtime
