# WhatsApp Bot (Rust)

A feature-rich WhatsApp Bot built in Rust using the `whatsapp-rust` library. It operates autonomously in authorized groups, allowing users to convert images to high-quality stickers, convert stickers back to images, and bypass "view-once" media restrictions.

## Features

- **Persistent Authentication**: Powered by a local SQLite store (`creds/whatsapp.db`) to avoid re-scanning QR codes on restarts.
- **Group Restrictions**: Only operates in specific, allowed groups defined via environment variables.
- **Sticker Maker (`s`)**: Converts replied images or GIFs into static WebP stickers (512x512) natively in pure Rust.
- **Image Converter (`i`)**: Converts WebP stickers back into standard JPEG images.
- **View-Once Resender (`r`)**: Unwraps and resends view-once media as standard media directly to the chat.
- **Group ID Fetcher (`g`)**: A utility command to fetch the current group JID for configuration purposes.

## Requirements

- Rust (Nightly toolchain: `nightly-2026-04-05` specified in `rust-toolchain.toml`)
- Cargo

## Installation & Setup

1. **Clone the repository** and navigate to the project root.
2. **Setup the Environment Variables**:
   Copy `.env.example` to `.env` and configure your settings:
   ```bash
   cp .env.example .env
   ```
   _Edit `.env` to add your allowed groups:_
   ```env
   ALLOWED_GROUPS=123456789@g.us,987654321@g.us
   ADMIN_NUMBERS=1234567890@s.whatsapp.net
   ```
3. **Run the Bot**:
   ```bash
   cargo run
   ```
4. **Scan the QR Code**: On the first run, the terminal will display a QR code block. Open WhatsApp on your phone -> Linked Devices -> Link a Device, and scan the code.
   _(Subsequent runs will automatically connect using the persisted SQLite session)._

## Commands

All commands are invoked by sending the exact string or replying to media with the string.

- `g` - Prints the internal Group ID (JID) of the current chat. Use this to easily find the ID to whitelist in `.env`.
- `s` - Reply to an Image or GIF with `s` to generate a WhatsApp sticker.
- `i` - Reply to a Sticker with `i` to convert it back to a JPEG image.
- `r` - Reply to a "view-once" image or video to unpack and resend it as normal media.

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
