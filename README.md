# WhatsApp Bot (Rust)

A feature-rich WhatsApp Bot built in Rust using the `whatsapp-rust` library. It operates autonomously in authorized groups, allowing users to convert images, GIFs, and videos into stickers, convert stickers back into images or videos, bypass "view-once" media restrictions, and download YouTube videos.

## Features

- **Persistent Authentication**: Powered by a local SQLite store (`creds/whatsapp.db`) to avoid re-scanning QR codes on restarts.
- **Group Restrictions**: Only operates in specific, allowed groups defined via environment variables.
- **Sticker Maker (`s`)**: Converts replied images, GIFs, or videos into WebP stickers (512x512), producing static stickers for images and animated stickers for GIFs/videos.
- **Sticker Unpacker (`i`)**: Converts a replied sticker back to media — a static sticker becomes a JPEG image, and an animated sticker becomes an MP4 video.
- **View-Once Resender (`r`)**: Unwraps and resends view-once media as standard media directly to the chat.
- **Group ID Fetcher (`g`)**: A utility command to fetch the current group JID for configuration purposes.
- **Help Menu (`h`)**: Lists all available commands directly in chat.
- **YouTube Downloader (`d`)**: Downloads a YouTube video by URL (or from a replied message) and sends it back as a video, with a duration cap to avoid excessively long downloads. Requires a cookies file and a JS runtime (Deno) to work reliably — see [yt-dlp dependency](#yt-dlp-dependency) below.

## Requirements

- Rust (Nightly toolchain: `nightly-2026-04-05` specified in `rust-toolchain.toml`)
- Cargo
- **ffmpeg** installed and available on `PATH` (required for animated sticker ↔ video/GIF conversion — see [ffmpeg dependency](#ffmpeg-dependency) below)
- **yt-dlp** binary (auto-downloaded on first run into `./libs` — required for the `d` command)
- **Deno** installed and available on `PATH` (required for the `d` command to reliably fetch real video/audio formats — see [yt-dlp dependency](#yt-dlp-dependency) below)
- **A YouTube cookies file** (required for the `d` command on most videos — see [yt-dlp dependency](#yt-dlp-dependency) below)

## Installation & Setup

1. **Clone the repository** and navigate to the project root.
2. **Install ffmpeg** (see [ffmpeg dependency](#ffmpeg-dependency) — required on both your dev machine and any deploy target).
3. **Install Deno and set up YouTube cookies** (see [yt-dlp dependency](#yt-dlp-dependency) — required for the `d` command).
4. **Setup the Environment Variables**:
   Copy `.env.example` to `.env` and configure your settings:
   ```bash
   cp .env.example .env
   ```
   _Edit `.env` to add your allowed groups:_
   ```env
   ALLOWED_GROUPS=123456789@g.us,987654321@g.us
   ADMIN_NUMBERS=1234567890@s.whatsapp.net
   ```
   _Optionally override the default cookies file location:_
   ```env
   YTDLP_COOKIES_FILE=creds/youtube_cookies.txt
   ```
5. **Run the Bot**:
   ```bash
   cargo run
   ```
6. **Scan the QR Code**: On the first run, the terminal will display a QR code block. Open WhatsApp on your phone -> Linked Devices -> Link a Device, and scan the code.
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
  - Before downloading, the bot checks the video's duration via `yt-dlp` metadata (no media is fetched for this check). Videos longer than **`MAX_VIDEO_DURATION_SECS`** (default: **3600 seconds / 1 hour**, set in `src/commands/public.rs`) are rejected with a message asking for a shorter video, instead of spending time and bandwidth downloading something huge. If duration can't be determined for some reason, the bot logs a warning and attempts the download anyway rather than blocking it outright.
  - Requires ffmpeg (for merging), Deno (for solving YouTube's JS challenge), and a valid cookies file (for bot-detection bypass) to be set up correctly — see below.

### ffmpeg dependency

Any conversion involving video or animated stickers (the video branch of `s`, the animated branch of `i`, and merging streams for `d`) shells out to `ffmpeg` via `ffmpeg-sidecar` or directly via `yt-dlp`. This requires ffmpeg with `libx264` and `libwebp` support to be installed **on whichever machine actually runs the bot process** — dev and deploy environments are independent, so if you develop on Windows and deploy on Linux, install it on both:

- **Windows**: `winget install ffmpeg` (or download a build from gyan.dev and add it to `PATH`)
- **Linux (Debian/Ubuntu)**: `sudo apt install ffmpeg`

Verify it's on `PATH` with `ffmpeg -version` and `ffmpeg -encoders | grep 264` (confirms `libx264` is available) in a fresh terminal/shell before running the bot.

If ffmpeg isn't found, the bot will attempt `ffmpeg-sidecar`'s `auto_download()` as a fallback and log a warning — but this can be slow or stall entirely depending on network access, so a manual install is strongly recommended for reliable operation rather than relying on this fallback.

Conversions involving ffmpeg run on a blocking thread pool with timeouts (60s for sticker-from-image/GIF, 240s for video/animated-sticker conversions) so a slow or stuck ffmpeg process can't hang the bot's message loop.

### yt-dlp dependency

The `d` command shells out directly to the standalone [`yt-dlp`](https://github.com/yt-dlp/yt-dlp) binary as a subprocess, rather than using a Rust wrapper crate. On first run, the bot automatically downloads the correct binary for your OS into `./libs` and reuses it on subsequent runs (self-updating on every startup via `yt-dlp -U`, since YouTube's extraction internals change frequently enough that a stale binary can silently break downloads).

We deliberately avoid the `yt-dlp` Rust crate (`boul2gom/yt-dlp`): as of writing, its published `2.7.2` release depends on `lofty ^0.23.2`, and both existing releases in that range have been yanked from crates.io, making the crate impossible to compile fresh (tracked upstream in [boul2gom/yt-dlp#192](https://github.com/boul2gom/yt-dlp/issues/192), still unresolved). Invoking the `yt-dlp` binary directly sidesteps this entirely, and also avoids depending on a large stack of features (caching, webhooks, audio metadata tagging) that this bot doesn't use.

Reliable operation of the `d` command needs **two additional pieces of setup** beyond just having the `yt-dlp` binary:

#### 1. A JavaScript runtime (Deno)

As of late 2025, YouTube requires solving a JavaScript-based anti-throttling puzzle ("n challenge") to obtain real video/audio format URLs. Without a JS runtime, `yt-dlp` falls back to only storyboard/thumbnail formats and downloads will fail with `"Requested format is not available"`. Install [Deno](https://deno.land/) (yt-dlp's recommended/default runtime) on **whichever machine runs the bot**:

```bash
curl -fsSL https://deno.land/install.sh | sh
echo 'export PATH="$HOME/.deno/bin:$PATH"' >> ~/.bashrc
source ~/.bashrc
deno --version
```

**If running as a systemd service**, note that `~/.bashrc` changes only apply to interactive shells — systemd units use their own environment. Add Deno's path explicitly to your unit file:

```ini
[Service]
Environment="PATH=/home/youruser/.deno/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin"
```

Then `sudo systemctl daemon-reload && sudo systemctl restart <your-service-name>`.

You can verify this is working correctly on the server with:
```bash
./libs/yt-dlp --remote-components ejs:github --cookies creds/youtube_cookies.txt -F "https://youtu.be/dQw4w9WgXcQ"
```
This should list real video/audio formats, not just `mhtml` storyboard entries.

#### 2. A YouTube cookies file

Many videos additionally require an authenticated session to bypass YouTube's bot-detection (`"Sign in to confirm you're not a bot"`). This requires exporting cookies from a real, logged-in YouTube session:

1. Read yt-dlp's own guide first: [How do I pass cookies to yt-dlp](https://github.com/yt-dlp/yt-dlp/wiki/FAQ#how-do-i-pass-cookies-to-yt-dlp) and [Exporting YouTube cookies](https://github.com/yt-dlp/yt-dlp/wiki/Extractors#exporting-youtube-cookies) for the most current, detailed instructions and recommended browser extensions.
2. **Use a secondary/throwaway Google account for this**, not your personal one — the exported file is equivalent to a logged-in session for that account, and treating it as disposable means nothing personally sensitive is at stake if it needs rotating.
3. Export cookies in Netscape format (the extensions linked in yt-dlp's wiki do this automatically) and place the file on the server at `creds/youtube_cookies.txt` (or wherever `YTDLP_COOKIES_FILE` in `.env` points).
4. **Never commit this file to git.** Make sure your `.gitignore` includes:
   ```
   creds/
   libs/
   downloads/
   ```
5. Copy it to the server out-of-band (e.g. `scp`), not through version control:
   ```bash
   scp youtube_cookies.txt user@your-server:/path/to/wa-bot-rust/creds/youtube_cookies.txt
   ```

On startup, the bot logs whether it found a cookies file at the resolved path. If it's missing, `d` will still attempt downloads but will likely fail on bot-detection-protected videos with the "Sign in to confirm you're not a bot" error.

**Cookies expire.** Several of the exported cookie values (especially `__Secure-1PSIDTS`, `__Secure-3PSIDTS`, `SIDCC`) are short-lived rotating tokens that a real browser refreshes automatically during normal use — a static exported file can go stale within days to weeks even though the longer-lived `SID`/`PSID` values last longer. If `d` starts failing again with a "sign in" error after previously working, re-export a fresh cookies file rather than assuming it's a code regression.

⚠️ **Security note**: a YouTube cookies file is tied to a real Google account session — treat it exactly like a credential. Anyone with the file can access that account's YouTube session (and potentially other Google services under the same session) until it's rotated or the account's password is changed.

---

## ⚠️ Known Issues & Workarounds

### Admin Privilege / Phone Number Identifier

There is currently a slight issue with reliably matching the phone number identifiers for the Admin privilege check. Because of this, **the `r` (view-once resend) command has been temporarily moved to a public act**.

Any user in an authorized group can currently use the `r` command until the admin identifier matching logic is patched and re-enabled in `src/handlers.rs`.

### YouTube Downloader (`d`) — Requires Ongoing Maintenance

Unlike the other commands, `d` depends on external, frequently-changing factors outside this bot's control:

- **YouTube's bot-detection and JS-challenge requirements evolve over time.** The current setup (Deno + `--remote-components ejs:github` + a cookies file) reflects YouTube/yt-dlp's requirements as of this writing, but YouTube has changed its extraction requirements multiple times in the past and is expected to continue doing so. If `d` starts failing again, check the [yt-dlp EJS wiki page](https://github.com/yt-dlp/yt-dlp/wiki/EJS) and the [yt-dlp FAQ](https://github.com/yt-dlp/yt-dlp/wiki/FAQ) for updated guidance before assuming it's a bug in this bot.
- **Cookies expire** and need periodic re-export (see [yt-dlp dependency](#yt-dlp-dependency) above).
- **Some videos may still fail** even with everything set up correctly — livestreams, age-restricted content, region-locked videos, and members-only content have additional restrictions that cookies/Deno alone may not resolve.

## ⚖️ Ethical & Privacy Disclaimer

### View-Once Media Automation

This bot includes a feature (`r`) to unpack and resend "view-once" media. While circumventing the sender's intended privacy controls raises ethical considerations, it is important to understand the technical realities of the platform:

- **Intended Audience**: If a message was sent to you (or your authorized group), the platform has already delivered that data to you. You are going to see it anyway.
- **No Absolute Security**: "View once" is a casual deterrent for accidental sharing, not a foolproof security mechanism. Any recipient can still photograph the screen or use modified clients. No messaging platform can completely prevent a recipient from capturing information they already have access to.

By using this bot, you acknowledge these technical realities. Please use this tool responsibly and consider the sender's expectations when automating the capture of restricted media.

### YouTube Downloader

The `d` command downloads publicly (or account-) accessible YouTube content for personal use in the chat it's invoked from. Downloading and redistributing copyrighted content may violate YouTube's Terms of Service and applicable copyright law depending on your jurisdiction and how the downloaded media is subsequently used or shared. Use responsibly and at your own discretion.