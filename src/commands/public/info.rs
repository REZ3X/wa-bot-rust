use whatsapp_rust::prelude::*;

const CARGO_TOML: &str = include_str!("../../../Cargo.toml");
const RUST_TOOLCHAIN_TOML: &str = include_str!("../../../rust-toolchain.toml");

fn extract_toml_string_value<'a>(toml: &'a str, key: &str) -> &'a str {
    toml.lines()
        .find_map(|line| {
            let line = line.trim();
            line.strip_prefix(key)
                .and_then(|rest| rest.trim_start().strip_prefix('='))
                .map(|value| value.trim().trim_matches('"'))
        })
        .unwrap_or("unknown")
}

pub async fn handle_t(ctx: &MessageContext) {
    // CARGO_PKG_NAME / CARGO_PKG_VERSION are populated automatically by Cargo
    // from Cargo.toml at build time. Cargo does NOT expose an edition env
    // var, so that (and the toolchain channel) are read from the manifest
    // files directly at compile time instead.
    let name = env!("CARGO_PKG_NAME");
    let version = env!("CARGO_PKG_VERSION");
    let edition = extract_toml_string_value(CARGO_TOML, "edition");
    let channel = extract_toml_string_value(RUST_TOOLCHAIN_TOML, "channel");

    let tech_text = format!(
        "Project: {name} `v{version}`\n\
         \n\
         Techstack Used:\n\
         Rust {edition} Edition\n\
         Cargo\n\
         Toolchain `{channel}` channel\n\
         FFmpeg\n\
         yt-dlp\n\
         whatsapp-rust\n\
         Deno\n\
         Tokio\n\
         SQLite\n\
         \n\
         For more information, see the project repository:\n\
         https://github.com/REZ3X/wa-bot-rust
         "
    );

    let _ = ctx.reply_quoting(&tech_text).await;
}

pub async fn handle_c(ctx: &MessageContext) {
    let chat = &ctx.info.source.chat;
    let _ = ctx.reply_quoting(&chat.to_string()).await;
}

pub async fn handle_h(ctx: &MessageContext) {
    let help_text =
        "Available commands:\n\
                     c - Get chat JID (works in groups and DMs)\n\
                     h - Show this help message\n\
                     s - Convert image/video to sticker (reply to media)\n\
                     i - Convert sticker to image/video (reply to sticker)\n\
                     r - Resend view-once media (reply to view-once message)\n\
                     t - Show techstack used to build this bot\n\
                     d - Download YouTube video ('d <url>' or reply with 'd' to a message containing a YouTube URL, maximum 1 hour of duration)\n\
                     ";
    let _ = ctx.reply_quoting(help_text).await;
}
