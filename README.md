# WhatsApp Bot (Rust)

A feature-rich WhatsApp Bot built in Rust using the `whatsapp-rust` library. It operates autonomously in authorized groups, allowing users to convert images, GIFs, and videos into stickers, convert stickers back into images or videos, and bypass "view-once" media restrictions.

## Features

- **Persistent Authentication**: Powered by a local SQLite store (`creds/whatsapp.db`) to avoid re-scanning QR codes on restarts.
- **Group Restrictions**: Only operates in specific, allowed groups defined via environment variables.
- **Sticker Maker (`s`)**: Converts replied images, GIFs, or videos into WebP stickers (512x512), producing static stickers for images and animated stickers for GIFs/videos.
- **Sticker Unpacker (`i`)**: Converts a replied sticker back to media — a static sticker becomes a JPEG image, and an animated sticker becomes an MP4 video.
- **View-Once Resender (`r`)**: Unwraps and resends view-once media as standard media directly to the chat.
- **Group ID Fetcher (`g`)**: A utility command to fetch the current group JID for configuration purposes.
- **Help Menu (`h`)**: Lists all available commands directly in chat.
- **YouTube Downloader (`d`)**: Downloads a YouTube video by URL (or from a replied message) and sends it back as a video. ⚠️ Currently broken for many videos — see [Known Issues](#known-issues--workarounds).

## Requirements

- Rust (Nightly toolchain: `nightly-2026-04-05` specified in `rust-toolchain.toml`)
- Cargo
- **ffmpeg** installed and available on `PATH` (required for animated sticker ↔ video/GIF conversion — see [ffmpeg dependency](#ffmpeg-dependency) below)
- **yt-dlp** binary (auto-downloaded on first run into `./libs` — required for the `d` command, see [yt-dlp dependency](#yt-dlp-dependency) below)

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
- `h` - Shows a help message listing all available commands.
- `d` - Downloads a YouTube video and sends it back as a video message. Usage:
  - `d <YouTube URL>` — e.g. `d https://youtu.be/dQw4w9WgXcQ`
  - Reply to a message containing a YouTube URL with just `d`.
  - ⚠️ **Currently broken for many videos** due to YouTube bot-detection — see [Known Issues](#known-issues--workarounds).

### ffmpeg dependency

Any conversion involving video or animated stickers (the video branch of `s`, and the animated branch of `i`) shells out to `ffmpeg` via `ffmpeg-sidecar`. This requires ffmpeg with `libx264` and `libwebp` support to be installed **on whichever machine actually runs the bot process** — dev and deploy environments are independent, so if you develop on Windows and deploy on Linux, install it on both:

- **Windows**: `winget install ffmpeg` (or download a build from gyan.dev and add it to `PATH`)
- **Linux (Debian/Ubuntu)**: `sudo apt install ffmpeg`

Verify it's on `PATH` with `ffmpeg -version` and `ffmpeg -encoders | grep 264` (confirms `libx264` is available) in a fresh terminal/shell before running the bot.

If ffmpeg isn't found, the bot will attempt `ffmpeg-sidecar`'s `auto_download()` as a fallback and log a warning — but this can be slow or stall entirely depending on network access, so a manual install is strongly recommended for reliable operation rather than relying on this fallback.

Conversions involving ffmpeg run on a blocking thread pool with timeouts (60s for sticker-from-image/GIF, 240s for video/animated-sticker conversions) so a slow or stuck ffmpeg process can't hang the bot's message loop.

### yt-dlp dependency

The `d` command shells out directly to the standalone [`yt-dlp`](https://github.com/yt-dlp/yt-dlp) binary as a subprocess, rather than using a Rust wrapper crate. On first run, the bot automatically downloads the correct binary for your OS into `./libs` and reuses it on subsequent runs. This requires outbound network access to GitHub the first time the bot starts (same one-time requirement as ffmpeg's auto-download fallback).

We deliberately avoid the `yt-dlp` Rust crate (`boul2gom/yt-dlp`) here: as of writing, its published `2.7.2` release depends on `lofty ^0.23.2`, and both existing releases in that range have been yanked from crates.io, making the crate impossible to compile fresh (tracked upstream in [boul2gom/yt-dlp#192](https://github.com/boul2gom/yt-dlp/issues/192), still unresolved). Invoking the `yt-dlp` binary directly sidesteps this entirely, and also avoids depending on a large stack of features (caching, webhooks, audio metadata tagging) that this bot doesn't use.

Video download/merge uses your system-installed ffmpeg automatically (same one required above), so no separate ffmpeg setup is needed for the `d` command specifically.

---

## ⚠️ Known Issues & Workarounds

### Admin Privilege / Phone Number Identifier

There is currently a slight issue with reliably matching the phone number identifiers for the Admin privilege check. Because of this, **the `r` (view-once resend) command has been temporarily moved to a public act**.

Any user in an authorized group can currently use the `r` command until the admin identifier matching logic is patched and re-enabled in `src/handlers.rs`.

### YouTube Downloader (`d`) — Currently Broken for Many Videos

The `d` command frequently fails with an error like:

```
Failed to download video: yt-dlp exited with exit status: 1: ERROR: [youtube] VIDEO_ID: Sign in to confirm you're
not a bot. Use --cookies-from-browser or --cookies for the authentication.
```

**Cause**: YouTube has been rolling out stricter bot-detection that blocks anonymous (cookie-less) requests from `yt-dlp` on certain videos, IPs, and datacenter/VPS network ranges in particular (which is why this may be more likely to show up on a deployed server than on a home connection). This is not a bug in this bot's code — it's YouTube requiring an authenticated session before it will serve the video.

**Fix (not yet implemented)**: `yt-dlp` supports passing YouTube session cookies via `--cookies-from-browser <browser>` (read cookies from a local browser profile) or `--cookies <cookies-file>` (a Netscape-format cookies file, e.g. exported with a browser extension). This isn't yet wired into `download_youtube_video` in `src/commands/public.rs` — it currently calls `yt-dlp` with no cookie authentication at all. Planned fix: export cookies once from a logged-in YouTube session, store the cookies file securely alongside the bot (excluded from git via `.gitignore`), and pass `--cookies <path>` in the `yt-dlp` invocation. Until then, expect the `d` command to fail intermittently or consistently depending on the video and the server's network reputation with YouTube.

See yt-dlp's own documentation for details: [How do I pass cookies to yt-dlp](https://github.com/yt-dlp/yt-dlp/wiki/FAQ#how-do-i-pass-cookies-to-yt-dlp) and [Exporting YouTube cookies](https://github.com/yt-dlp/yt-dlp/wiki/Extractors#exporting-youtube-cookies).

⚠️ **Security note for later**: a YouTube cookies file is tied to a real Google account session. Treat it like a credential — never commit it to version control, and be aware that sharing it grants access to that account's YouTube session.

## ⚖️ Ethical & Privacy Disclaimer

### View-Once Media Automation

This bot includes a feature (`r`) to unpack and resend "view-once" media. While circumventing the sender's intended privacy controls raises ethical considerations, it is important to understand the technical realities of the platform:

- **Intended Audience**: If a message was sent to you (or your authorized group), the platform has already delivered that data to you. You are going to see it anyway.
- **No Absolute Security**: "View once" is a casual deterrent for accidental sharing, not a foolproof security mechanism. Any recipient can still photograph the screen or use modified clients. No messaging platform can completely prevent a recipient from capturing information they already have access to.

By using this bot, you acknowledge these technical realities. Please use this tool responsibly and consider the sender's expectations when automating the capture of restricted media.