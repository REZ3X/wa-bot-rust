use std::path::PathBuf;
use std::time::Duration;

use anyhow::Context;
use tempfile::tempdir;
use whatsapp_rust::prelude::*;
use whatsapp_rust::upload::UploadOptions;
use whatsapp_rust::wacore::download::MediaType;

const MAX_VIDEO_DURATION_SECS: u64 = 3600;

pub struct YtDlpContext {
    pub binary_path: PathBuf,
    pub cookies_path: Option<PathBuf>,
}

fn find_youtube_url(text: &str) -> Option<String> {
    text.split_whitespace()
        .find(|word| word.contains("youtube.com/") || word.contains("youtu.be/"))
        .map(|s| s.trim().to_string())
}

/// Fetches only the video's duration (in seconds) without downloading any
/// media streams, so we can reject overly long videos before spending time
/// and bandwidth on an actual download.
async fn fetch_video_duration(ctx: &YtDlpContext, url: &str) -> anyhow::Result<u64> {
    let mut cmd = tokio::process::Command::new(&ctx.binary_path);
    cmd.args([
        "--print",
        "%(duration)s",
        "--skip-download",
        "--no-playlist",
        "--no-warnings",
        "--remote-components",
        "ejs:github",
    ]);

    if let Some(cookies) = &ctx.cookies_path {
        cmd.arg("--cookies").arg(cookies);
    }

    cmd.arg(url);

    let output = cmd.output().await.context("failed to spawn yt-dlp for duration check")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("yt-dlp exited with {}: {}", output.status, stderr.trim());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let duration_str = stdout.trim();

    duration_str
        .parse::<f64>()
        .map(|d| d.round() as u64)
        .with_context(|| format!("failed to parse duration from yt-dlp output: '{duration_str}'"))
}

/// Downloads a YouTube video by shelling out to the yt-dlp binary directly.
async fn download_youtube_video(ctx: &YtDlpContext, url: String) -> anyhow::Result<Vec<u8>> {
    let workdir = tempdir().context("failed to create temporary directory")?;
    let output_path = workdir.path().join("video.mp4");

    log::info!("download_youtube_video: fetching {url}");

    let mut cmd = tokio::process::Command::new(&ctx.binary_path);
    cmd.args([
        "-f",
        "bv*+ba/b",
        "-S",
        "vcodec:h264,acodec:m4a,res:1080",
        "--merge-output-format",
        "mp4",
        "--no-playlist",
        "--no-warnings",
        "--remote-components",
        "ejs:github",
    ]);

    if let Some(cookies) = &ctx.cookies_path {
        cmd.arg("--cookies").arg(cookies);
    }

    cmd.arg("-o").arg(&output_path).arg(&url);

    let output = cmd.output().await.context("failed to spawn yt-dlp")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("yt-dlp exited with {}: {}", output.status, stderr.trim());
    }

    tokio::fs::read(&output_path).await.context("failed to read downloaded video")
}

pub async fn handle_d(ctx: &MessageContext, ytdlp: &YtDlpContext) {
    let text = ctx.message
        .text_content()
        .or_else(|| ctx.message.get_caption())
        .unwrap_or_default()
        .trim()
        .to_string();

    let url = if text.starts_with("d ") {
        find_youtube_url(text[2..].trim())
    } else if text == "d" {
        let mut quoted_url = None;
        if let Some(ext) = ctx.message.extended_text_message.as_option() {
            if let Some(ctx_info) = ext.context_info.as_option() {
                if let Some(quoted) = ctx_info.quoted_message.as_option() {
                    if let Some(conv) = quoted.conversation.as_ref() {
                        quoted_url = find_youtube_url(conv);
                    }
                    if quoted_url.is_none() {
                        if let Some(ext_text) = quoted.extended_text_message.as_option() {
                            if let Some(text) = ext_text.text.as_ref() {
                                quoted_url = find_youtube_url(text);
                            }
                        }
                    }
                }
            }
        }
        quoted_url
    } else {
        None
    };

    let url = match url {
        Some(u) => u,
        None => {
            let _ = ctx.reply_quoting(
                "Usage: 'd <YouTube URL>' or reply to a message containing a YouTube URL with 'd'."
            ).await;
            return;
        }
    };

    let _ = ctx.reply_quoting("Checking video info...").await;

    match tokio::time::timeout(Duration::from_secs(30), fetch_video_duration(ytdlp, &url)).await {
        Ok(Ok(duration_secs)) => {
            if duration_secs > MAX_VIDEO_DURATION_SECS {
                let minutes = duration_secs / 60;
                let max_minutes = MAX_VIDEO_DURATION_SECS / 60;
                log::info!("handle_d: rejecting video, duration {duration_secs}s exceeds limit");
                let _ = ctx.reply_quoting(
                    &format!(
                        "This video is about {minutes} minute(s) long, which exceeds the {max_minutes}-minute limit for downloads. Please choose a shorter video."
                    )
                ).await;
                return;
            }
            log::info!("handle_d: video duration {duration_secs}s is within limit, proceeding");
        }
        Ok(Err(error)) => {
            // Couldn't determine duration (e.g. live stream, age-gated video,
            // or a transient extractor issue) — log it but proceed anyway
            // rather than blocking legitimate downloads on a metadata hiccup.
            log::warn!("handle_d: failed to fetch video duration, proceeding anyway: {error:?}");
        }
        Err(_) => {
            log::warn!("handle_d: duration check timed out after 30s, proceeding anyway");
        }
    }

    let _ = ctx.reply_quoting("Downloading video, please wait...").await;

    let download_result = tokio::time::timeout(
        Duration::from_secs(180),
        download_youtube_video(ytdlp, url.clone())
    ).await;

    match download_result {
        Ok(Ok(video_data)) => {
            log::info!("handle_d: downloaded {} bytes, uploading", video_data.len());
            match ctx.client.upload(video_data, MediaType::Video, UploadOptions::default()).await {
                Ok(upload) => {
                    let mut reply = wa::Message {
                        video_message: buffa::MessageField::some(wa::message::VideoMessage {
                            url: Some(upload.url),
                            direct_path: Some(upload.direct_path),
                            media_key: Some(upload.media_key.to_vec()),
                            file_sha256: Some(upload.file_sha256.to_vec()),
                            file_enc_sha256: Some(upload.file_enc_sha256.to_vec()),
                            file_length: Some(upload.file_length),
                            media_key_timestamp: Some(upload.media_key_timestamp),
                            mimetype: Some("video/mp4".to_string()),
                            ..Default::default()
                        }),
                        ..Default::default()
                    };
                    reply.set_context_info(ctx.build_quote_context());
                    let _ = ctx.send_message(reply).await;
                }
                Err(e) => {
                    log::error!("handle_d: video upload failed: {e:?}");
                    let _ = ctx.reply_quoting("Failed to upload video.").await;
                }
            }
        }
        Ok(Err(e)) => {
            log::error!("handle_d: download failed: {e:?}");
            let _ = ctx.reply_quoting(&format!("Failed to download video: {e}")).await;
        }
        Err(_) => {
            let _ = ctx.reply_quoting(
                "Download timed out. The video may be too large or restricted."
            ).await;
        }
    }
}
