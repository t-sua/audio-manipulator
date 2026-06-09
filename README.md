# Audio Manipulator

A lightweight desktop audio player and waveform editor built with Rust and [egui](https://github.com/emilk/egui).

## Features

- **Waveform visualization** — full-height overview with fast bucket-based rendering
- **Playback controls** — play/pause, stop, return to start, loop mode
- **Selection** — click and drag to select a region; looping and playback respect the selection
- **Zoom & pan** — Ctrl+scroll to zoom, scroll to pan, pinch-to-zoom on trackpads; zoom buttons and Fit/Sel→View shortcuts
- **Speed control** — logarithmic slider from 0.05× to 2× with OLA time-stretching
- **Volume control** — 0–100% slider
- **Color schemes** — customizable colors for the waveform, playhead, selection, and background; two built-in presets with save/load support and persistence across launches

## Supported formats

MP3, FLAC, WAV, OGG, AAC, M4A, Opus (via [Symphonia](https://github.com/pdeljanov/Symphonia))

## Controls

| Action | Input |
|---|---|
| Play / Pause | `Space` or Play button |
| Seek | Click on waveform |
| Select region | Click and drag |
| Clear selection | Double-click or Deselect button |
| Zoom in/out | `Ctrl` + scroll, or zoom buttons |
| Pan | Scroll (horizontal or vertical) |
| Open color editor | 🎨 Colors… button |

## Building

Requires [Rust](https://rustup.rs/) (stable).

```sh
cargo build --release
```

Run directly:

```sh
cargo run --release
```

### Linux

A working audio output device (ALSA/PulseAudio/PipeWire) and a display server (X11 or Wayland) are required. The file dialog uses the XDG portal — `xdg-desktop-portal` should be running on your session.

### Windows

Build natively with the MSVC toolchain:

```sh
rustup target add x86_64-pc-windows-msvc
cargo build --release --target x86_64-pc-windows-msvc
```

The app runs without a console window and uses the native Win32 file picker.

### macOS

```sh
cargo build --release
```

Uses the native macOS file picker automatically.

## License

MIT
