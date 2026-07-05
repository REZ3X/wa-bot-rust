mod config;
mod commands;
mod handlers;

use anyhow::Context;
use config::Config;
use log::{error, info};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use whatsapp_rust::prelude::*;

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

/// Downloads and caches the standalone yt-dlp binary under `libs/`, mirroring
/// how ffmpeg-sidecar auto-installs ffmpeg. We invoke yt-dlp as a subprocess
/// rather than depending on the `yt-dlp` Rust wrapper crate, since that crate
/// currently has a broken transitive dependency (lofty ^0.23.2, both existing
/// 0.23.x releases yanked from crates.io) and pulls in far more than we need.
async fn ensure_ytdlp_binary(libs_dir: &Path) -> anyhow::Result<PathBuf> {
    std::fs::create_dir_all(libs_dir).context("failed to create libs directory")?;

    let bin_name = if cfg!(windows) { "yt-dlp.exe" } else { "yt-dlp" };
    let bin_path = libs_dir.join(bin_name);

    if bin_path.exists() {
        return Ok(bin_path);
    }

    let asset = if cfg!(windows) {
        "yt-dlp.exe"
    } else if cfg!(target_os = "macos") {
        "yt-dlp_macos"
    } else {
        "yt-dlp"
    };
    let download_url = format!("https://github.com/yt-dlp/yt-dlp/releases/latest/download/{asset}");

    info!("yt-dlp binary not found, downloading from {download_url}");
    let response = reqwest::get(&download_url).await.context("failed to request yt-dlp binary")?;
    if !response.status().is_success() {
        anyhow::bail!("failed to download yt-dlp binary: HTTP {}", response.status());
    }
    let bytes = response.bytes().await.context("failed to read yt-dlp binary response")?;

    let mut file = std::fs::File::create(&bin_path).context("failed to create yt-dlp binary file")?;
    file.write_all(&bytes).context("failed to write yt-dlp binary")?;

    #[cfg(unix)]
    {
        let mut perms = std::fs::metadata(&bin_path)?.permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&bin_path, perms)?;
    }

    info!("yt-dlp binary installed at {}", bin_path.display());
    Ok(bin_path)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let config = Arc::new(Config::load());
    info!("Configuration loaded");

    // Persist session to local directory "creds"
    let _ = std::fs::create_dir_all("creds");
    let store = SqliteStore::new("creds/whatsapp.db").await?;
    info!("SQLite backend initialized");

    // Set up the yt-dlp binary once at startup (cached under ./libs so it
    // isn't re-downloaded on every restart). System ffmpeg (already required
    // for the sticker pipeline) is used automatically by yt-dlp when merging
    // separate audio/video streams, since it's already on PATH.
    let ytdlp_path = Arc::new(ensure_ytdlp_binary(Path::new("libs")).await?);
    info!("yt-dlp ready at {}", ytdlp_path.display());

    let bot = Bot::builder()
        .with_backend(store)
        .on_qr_code(|code, _timeout| async move {
            println!("\nScan this QR code with WhatsApp:");
            if let Err(e) = qr2term::print_qr(&code) {
                error!("Failed to print QR code: {e}");
                println!("Raw QR String: {code}");
            }
            println!();
        })
        .on_connected(|_client| async {
            info!("Bot connected successfully!");
        })
        .on_logged_out(|_info| async {
            error!("Bot was logged out!");
        })
        .on_message({
            let config = config.clone();
            let ytdlp_path = ytdlp_path.clone();
            move |ctx| {
                let config = config.clone();
                let ytdlp_path = ytdlp_path.clone();
                async move {
                    handlers::handle_message(ctx, config, ytdlp_path).await;
                }
            }
        })
        .build()
        .await?;

    info!("Starting bot...");
    bot.run().await;
    Ok(())
}