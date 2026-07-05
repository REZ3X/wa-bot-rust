# WhatsApp Bot (Rust)

A feature-rich WhatsApp Bot built in Rust using the `whatsapp-rust` library. It operates autonomously in authorized groups, allowing users to convert images, GIFs, and videos into stickers, convert stickers back into images or videos, and bypass "view-once" media restrictions.

## Features

- **Persistent Authentication**: Powered by a local SQLite store (`creds/whatsapp.db`) to avoid re-scanning QR codes on restarts.
- **Group Restrictions**: Only operates in specific, allowed groups defined via environment variables.
- **Sticker Maker (`s`)**: Converts replied images, GIFs, or videos into WebP stickers (512x512), producing static stickers for images and animated stickers for GIFs/videos.
- **Sticker Unpacker (`i`)**: Converts a replied sticker back to media — a static sticker becomes a JPEG image, and an animated sticker becomes an MP4 video.
- **View-Once Resender (`r`)**: Unwraps and resends view-once media as standard media directly to the chat.
- **Group ID Fetcher (`g`)**: A utility command to fetch the current group JID for configuration purposes.

## Requirements

- Rust (Nightly toolchain: `nightly-2026-04-05` specified in `rust-toolchain.toml`)
- Cargo
- **ffmpeg** installed and available on `PATH` (required for animated sticker ↔ video/GIF conversion — see [ffmpeg dependency](#ffmpeg-dependency) below)

## Installation & Setup

1. **Clone the repository** and navigate to the project root.
2. **Install ffmpeg** (see [ffmpeg dependency](#ffmpeg-dependency) — required on both your dev machine and any deploy target).
3. **Setup the Environment Variables**:
   Copy `.env.example` to `.env` and configure your settings:
   ```bash
   cp .env.example .env
   ```
   _Edit `.env` to add your allowed groups:_
   ```env
   ALLOWED_GROUPS=123456789@g.us,987654321@g.us
   ADMIN_NUMBERS=1234567890@s.whatsapp.net
   ```
4. **Run the Bot**:
   ```bash
   cargo run
   ```
5. **Scan the QR Code**: On the first run, the terminal will display a QR code block. Open WhatsApp on your phone -> Linked Devices -> Link a Device, and scan the code.
   _(Subsequent runs will automatically connect using the persisted SQLite session)._

## Commands

All commands are invoked by sending the exact string or replying to media with the string.

- `g` - Prints the internal Group ID (JID) of the current chat. Use this to easily find the ID to whitelist in `.env`.
- `s` - Reply to an Image, GIF, or Video with `s` to generate a WhatsApp sticker.
  - **Image** → static WebP sticker, resized and letterboxed to fit a 512x512 canvas.
  - **GIF** → animated WebP sticker, decoded and re-encoded frame-by-frame in pure Rust (no ffmpeg needed for this path).
  - **Video** (or animated WebP input) → animated WebP sticker via ffmpeg, resampled to 15fps and scaled/padded to 512x512.
- `i` - Reply to a Sticker with `i` to convert it back to media.
  - **Static sticker** → JPEG image.
  - **Animated sticker** → MP4 video. Animation is detected directly from the WebP bitstream (not from the sender-provided flag), decoded frame-by-frame, and re-encoded to H.264 via ffmpeg. Animated stickers longer than ~300 frames (roughly 10-12s at typical sticker framerates) are rejected with a message asking for a shorter sticker, to avoid excessive conversion time.
- `r` - Reply to a "view-once" image or video to unpack and resend it as normal media.

### ffmpeg dependency

Any conversion involving video or animated stickers (the video branch of `s`, and the animated branch of `i`) shells out to `ffmpeg` via `ffmpeg-sidecar`. This requires ffmpeg with `libx264` and `libwebp` support to be installed **on whichever machine actually runs the bot process** — dev and deploy environments are independent, so if you develop on Windows and deploy on Linux, install it on both:

- **Windows**: `winget install ffmpeg` (or download a build from gyan.dev and add it to `PATH`)
- **Linux (Debian/Ubuntu)**: `sudo apt install ffmpeg`

Verify it's on `PATH` with `ffmpeg -version` and `ffmpeg -encoders | grep 264` (confirms `libx264` is available) in a fresh terminal/shell before running the bot.

If ffmpeg isn't found, the bot will attempt `ffmpeg-sidecar`'s `auto_download()` as a fallback and log a warning — but this can be slow or stall entirely depending on network access, so a manual install is strongly recommended for reliable operation rather than relying on this fallback.

Conversions involving ffmpeg run on a blocking thread pool with timeouts (60s for sticker-from-image/GIF, 240s for video/animated-sticker conversions) so a slow or stuck ffmpeg process can't hang the bot's message loop.

---

## ⚠️ Known Issues & Workarounds

### Admin Privilege / Phone Number Identifier

There is currently a slight issue with reliably matching the phone number identifiers for the Admin privilege check. Because of this, **the `r` (view-once resend) command has been temporarily moved to a public act**.

Any user in an authorized group can currently use the `r` command until the admin identifier matching logic is patched and re-enabled in `src/handlers.rs`.

## ⚖️ Ethical & Privacy Disclaimer

### View-Once Media Automation

This bot includes a feature (`r`) to unpack and resend "view-once" media. While circumventing the sender's intended privacy controls raises ethical considerations, it is important to understand the technical realities of the platform:

- **Intended Audience**: If a message was sent to you (or your authorized group), the platform has already delivered that data to you. You are going to see it anyway.
- **No Absolute Security**: "View once" is a casual deterrent for accidental sharing, not a foolproof security mechanism. Any recipient can still photograph the screen or use modified clients. No messaging platform can completely prevent a recipient from capturing information they already have access to.

By using this bot, you acknowledge these technical realities. Please use this tool responsibly and consider the sender's expectations when automating the capture of restricted media.