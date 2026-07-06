use std::io::Cursor;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use std::path::PathBuf;

use anyhow::Context;
use ffmpeg_sidecar::{ command::FfmpegCommand, download::auto_download };
use image::{ AnimationDecoder, GenericImageView, ImageDecoder, ImageFormat, Rgba, RgbaImage };
use rayon::prelude::*;
use tempfile::tempdir;
use whatsapp_rust::prelude::*;
use whatsapp_rust::upload::UploadOptions;
use whatsapp_rust::wacore::download::MediaType;

const STICKER_SIZE: u32 = 512;
const ANIMATED_STICKER_FPS: f32 = 15.0;
const FFMPEG_TIMEOUT_SECS: u64 = 60;
const STICKER_TO_VIDEO_TIMEOUT_SECS: u64 = 240; // animated stickers can have many frames
const MAX_STICKER_FRAMES: usize = 300; // ~10-12s of animation at typical sticker framerates

pub struct YtDlpContext {
    pub binary_path: PathBuf,
    pub cookies_path: Option<PathBuf>,
}

pub async fn handle_g(ctx: &MessageContext) {
    let chat = &ctx.info.source.chat;
    let _ = ctx.send_message(wa::Message {
        conversation: Some(chat.to_string()),
        ..Default::default()
    }).await;
}

pub async fn handle_h(ctx: &MessageContext) {
    let help_text = "Available commands:\n\
                     g - Get group JID\n\
                     h - Show this help message\n\
                     s - Convert image/video to sticker (reply to media)\n\
                     i - Convert sticker to image/video (reply to sticker)\n\
                     r - Resend view-once media (reply to view-once message)\n\
                     ~d - Download YouTube video ('d <url>' or reply with 'd' to a message containing a YouTube URL)~\n\
                     [d - Current; issues occured due to yt-dlp cookies setup not yet implemented]\n\
                     ";
    let _ = ctx.send_message(wa::Message {
        conversation: Some(help_text.to_string()),
        ..Default::default()
    }).await;
}

fn resize_into_sticker_canvas(image: &image::DynamicImage) -> image::DynamicImage {
    let (width, height) = image.dimensions();
    let scale = f32::min(
        (STICKER_SIZE as f32) / (width as f32),
        (STICKER_SIZE as f32) / (height as f32)
    );

    let target_width = (((width as f32) * scale).round() as u32).clamp(1, STICKER_SIZE);
    let target_height = (((height as f32) * scale).round() as u32).clamp(1, STICKER_SIZE);

    let resized = image
        .resize(target_width, target_height, image::imageops::FilterType::Lanczos3)
        .to_rgba8();

    let mut canvas = RgbaImage::from_pixel(STICKER_SIZE, STICKER_SIZE, Rgba([0, 0, 0, 0]));
    let offset_x = ((STICKER_SIZE - target_width) / 2) as i64;
    let offset_y = ((STICKER_SIZE - target_height) / 2) as i64;

    image::imageops::overlay(&mut canvas, &resized, offset_x, offset_y);
    image::DynamicImage::ImageRgba8(canvas)
}

fn image_to_webp(data: &[u8]) -> anyhow::Result<Vec<u8>> {
    let img = image::load_from_memory(data).context("failed to decode image")?;
    let img = resize_into_sticker_canvas(&img);

    let encoder = webp::Encoder
        ::from_image(&img)
        .map_err(|e| anyhow::anyhow!("WebP encoding error: {:?}", e))?;

    let webp = encoder.encode(80.0);
    Ok(webp.to_vec())
}

fn gif_to_webp(data: &[u8]) -> anyhow::Result<Vec<u8>> {
    let decoder = image::codecs::gif::GifDecoder::new(Cursor::new(data)).context(
        "failed to create gif decoder"
    )?;
    let (gif_width, gif_height) = decoder.dimensions();
    let frames = decoder.into_frames().collect_frames().context("failed to collect gif frames")?;

    log::info!("gif_to_webp: decoded {} frames ({gif_width}x{gif_height})", frames.len());

    let mut images = Vec::with_capacity(frames.len());
    let mut timestamps = Vec::with_capacity(frames.len());
    let mut elapsed_ms: i32 = 0;

    for frame in frames {
        let left = frame.left();
        let top = frame.top();
        let delay_ms = Duration::from(frame.delay())
            .as_millis()
            .clamp(10, i32::MAX as u128) as i32;

        let mut full_frame = RgbaImage::from_pixel(gif_width, gif_height, Rgba([0, 0, 0, 0]));
        image::imageops::overlay(&mut full_frame, &frame.into_buffer(), left as i64, top as i64);

        let composed = resize_into_sticker_canvas(&image::DynamicImage::ImageRgba8(full_frame));
        elapsed_ms = elapsed_ms.saturating_add(delay_ms);
        timestamps.push(elapsed_ms);
        images.push(composed);
    }

    let mut config = webp::WebPConfig
        ::new()
        .map_err(|_| anyhow::anyhow!("Failed to create WebP config"))?;
    config.lossless = 1;
    config.alpha_compression = 0;
    config.alpha_filtering = 0;
    config.quality = 80.0;

    let mut encoder = webp::AnimEncoder::new(STICKER_SIZE, STICKER_SIZE, &config);
    encoder.set_bgcolor([0, 0, 0, 0]);
    encoder.set_loop_count(0);

    for (image, timestamp) in images.iter().zip(timestamps.iter()) {
        let frame = webp::AnimFrame
            ::from_image(image, *timestamp)
            .map_err(|error| anyhow::anyhow!("Failed to create animated frame: {error}"))?;
        encoder.add_frame(frame);
    }

    let webp = encoder.encode();
    log::info!("gif_to_webp: encoded {} bytes", webp.len());
    Ok(webp.to_vec())
}

fn ensure_ffmpeg_available() -> anyhow::Result<()> {
    if !ffmpeg_sidecar::command::ffmpeg_is_installed() {
        log::warn!("ffmpeg not installed, attempting auto_download()");
        auto_download().context("failed to download ffmpeg")?;
        log::info!("ffmpeg auto_download() succeeded");
    }
    Ok(())
}

fn run_ffmpeg_command(mut child: ffmpeg_sidecar::child::FfmpegChild) -> anyhow::Result<()> {
    let mut log_lines: Vec<String> = Vec::new();

    for event in child.iter().context("failed to read ffmpeg output")? {
        match event {
            ffmpeg_sidecar::event::FfmpegEvent::Log(level, msg) => {
                log_lines.push(format!("[{level:?}] {msg}"));
            }
            ffmpeg_sidecar::event::FfmpegEvent::Error(err) => {
                log_lines.push(format!("[Error] {err}"));
            }
            _ => {}
        }
    }

    let status = child.wait().context("failed to wait for ffmpeg")?;
    if !status.success() {
        // Only dump the tail of ffmpeg's own output when something goes wrong.
        let tail: Vec<&String> = log_lines.iter().rev().take(20).collect();
        for line in tail.iter().rev() {
            log::error!("ffmpeg output: {line}");
        }
        anyhow::bail!("ffmpeg exited with status: {status}");
    }

    Ok(())
}

fn convert_video_to_webp(input_path: &Path, output_path: &Path) -> anyhow::Result<()> {
    ensure_ffmpeg_available()?;

    let filtergraph = format!(
        "fps={fps},scale={size}:{size}:force_original_aspect_ratio=decrease,pad={size}:{size}:(ow-iw)/2:(oh-ih)/2:color=0x00000000",
        fps = ANIMATED_STICKER_FPS,
        size = STICKER_SIZE
    );
    let input = input_path.to_string_lossy().to_string();
    let output = output_path.to_string_lossy().to_string();

    log::info!("convert_video_to_webp: input={input} output={output}");

    let child = FfmpegCommand::new()
        .hide_banner()
        .overwrite()
        .input(input)
        .filter(filtergraph)
        .no_audio()
        .pix_fmt("yuva420p")
        .codec_video("libwebp")
        .args(["-lossless", "0", "-q:v", "80", "-preset", "picture", "-loop", "0"])
        .output(output)
        .spawn()
        .context("failed to start ffmpeg")?;

    run_ffmpeg_command(child)?;
    log::info!("convert_video_to_webp: success");
    Ok(())
}

/// Decodes an animated WebP sticker into individual frames (mirrors gif_to_webp's
/// approach), since ffmpeg's own WebP decoder cannot read animated WebP (ANMF chunks).
/// Frame compositing + PNG encoding is parallelized across threads via rayon, since
/// the decode step (collect_frames) is the only inherently sequential/slow part.
fn animated_webp_to_mp4(data: &[u8]) -> anyhow::Result<Vec<u8>> {
    ensure_ffmpeg_available()?;

    let decoder = image::codecs::webp::WebPDecoder::new(Cursor::new(data)).context(
        "failed to create webp decoder"
    )?;
    let (width, height) = decoder.dimensions();
    let frames = decoder
        .into_frames()
        .collect_frames()
        .context("failed to decode animated webp frames")?;

    if frames.is_empty() {
        anyhow::bail!("animated webp contained no frames");
    }

    log::info!("animated_webp_to_mp4: decoded {} frames ({width}x{height})", frames.len());

    if frames.len() > MAX_STICKER_FRAMES {
        anyhow::bail!(
            "animated sticker has too many frames ({} > {}), refusing to convert",
            frames.len(),
            MAX_STICKER_FRAMES
        );
    }

    let workdir = tempdir().context("failed to create temporary directory")?;
    let workdir_path = workdir.path().to_path_buf();

    // Precompute per-frame metadata sequentially (cheap, preserves ordering),
    // then do the expensive composite+PNG-encode work in parallel across cores.
    let frame_meta: Vec<(usize, String, u64, i32, i32)> = frames
        .iter()
        .enumerate()
        .map(|(i, frame)| {
            let delay_ms = Duration::from(frame.delay()).as_millis().clamp(10, 60_000) as u64;
            (i, format!("frame_{i:05}.png"), delay_ms, frame.left() as i32, frame.top() as i32)
        })
        .collect();

    let last_index = frames.len() - 1;

    frame_meta
        .par_iter()
        .zip(frames.par_iter())
        .try_for_each(|((i, filename, _delay, left, top), frame)| -> anyhow::Result<()> {
            // Composite onto a full-size canvas in case this frame only covers a
            // sub-region (partial-frame updates), same as the GIF path does.
            let mut full_frame = RgbaImage::from_pixel(width, height, Rgba([0, 0, 0, 0]));
            image::imageops::overlay(&mut full_frame, frame.buffer(), *left as i64, *top as i64);

            let path = workdir_path.join(filename);
            full_frame
                .save_with_format(&path, image::ImageFormat::Png)
                .with_context(|| format!("failed to write frame {i}"))?;
            Ok(())
        })?;

    // Build the concat manifest sequentially afterward, since ordering matters here.
    let mut concat_manifest = String::new();
    for (i, filename, delay_ms, _left, _top) in &frame_meta {
        concat_manifest.push_str(&format!("file '{filename}'\n"));
        concat_manifest.push_str(&format!("duration {:.3}\n", (*delay_ms as f64) / 1000.0));

        // ffmpeg's concat demuxer ignores the duration on the final entry, so
        // the last frame must be listed once more without a duration line.
        if *i == last_index {
            concat_manifest.push_str(&format!("file '{filename}'\n"));
        }
    }

    let concat_path = workdir.path().join("concat.txt");
    std::fs::write(&concat_path, concat_manifest).context("failed to write concat manifest")?;

    let output_path = workdir.path().join("output.mp4");

    log::info!("animated_webp_to_mp4: encoding via concat demuxer -> {}", output_path.display());

    let child = FfmpegCommand::new()
        .hide_banner()
        .overwrite()
        .args(["-f", "concat", "-safe", "0"])
        .input(concat_path.to_string_lossy().to_string())
        .filter("scale=trunc(iw/2)*2:trunc(ih/2)*2,format=yuv420p")
        .codec_video("libx264")
        .args(["-preset", "veryfast", "-crf", "23", "-movflags", "+faststart"])
        .output(output_path.to_string_lossy().to_string())
        .spawn()
        .context("failed to start ffmpeg")?;

    run_ffmpeg_command(child)?;
    log::info!("animated_webp_to_mp4: success");

    std::fs::read(&output_path).context("failed to read generated video")
}

fn animated_video_to_webp(data: &[u8]) -> anyhow::Result<Vec<u8>> {
    let workdir = tempdir().context("failed to create temporary directory")?;
    let input_path = workdir.path().join("input.bin");
    let output_path = workdir.path().join("output.webp");

    std::fs::write(&input_path, data).context("failed to write input media")?;
    convert_video_to_webp(&input_path, &output_path)?;

    std::fs::read(&output_path).context("failed to read generated sticker")
}

fn convert_image_media_to_webp(data: &[u8]) -> anyhow::Result<(Vec<u8>, bool)> {
    match image::guess_format(data) {
        Ok(ImageFormat::Gif) => {
            log::info!("convert_image_media_to_webp: detected GIF");
            gif_to_webp(data).map(|webp| (webp, true))
        }
        Ok(ImageFormat::WebP) if
            webp::BitstreamFeatures
                ::new(data)
                .map(|features| features.has_animation())
                .unwrap_or(false)
        => {
            log::info!("convert_image_media_to_webp: detected animated WebP, routing through ffmpeg");
            animated_video_to_webp(data).map(|webp| (webp, true))
        }
        other => {
            log::info!("convert_image_media_to_webp: detected static format {other:?}");
            image_to_webp(data).map(|webp| (webp, false))
        }
    }
}

fn convert_video_media_to_webp(data: &[u8]) -> anyhow::Result<(Vec<u8>, bool)> {
    log::info!("convert_video_media_to_webp: starting ({} bytes)", data.len());
    animated_video_to_webp(data).map(|webp| (webp, true))
}

pub fn webp_to_image(data: &[u8]) -> anyhow::Result<Vec<u8>> {
    let img = image::load_from_memory(data)?;
    let mut buf = std::io::Cursor::new(Vec::new());
    img.write_to(&mut buf, image::ImageFormat::Jpeg)?;
    Ok(buf.into_inner())
}

/// Runs a blocking closure on a dedicated thread pool with a timeout,
/// so slow/hanging ffmpeg or image work never stalls the async runtime.
async fn run_blocking<F, T>(f: F, timeout_secs: u64) -> anyhow::Result<T>
    where F: FnOnce() -> anyhow::Result<T> + Send + 'static, T: Send + 'static
{
    let join_result = tokio::time::timeout(
        Duration::from_secs(timeout_secs),
        tokio::task::spawn_blocking(f)
    ).await;

    match join_result {
        Ok(Ok(inner)) => inner,
        Ok(Err(join_err)) => Err(anyhow::anyhow!("conversion task panicked: {join_err}")),
        Err(_) => Err(anyhow::anyhow!("conversion timed out after {timeout_secs}s")),
    }
}

pub async fn handle_s(ctx: &MessageContext) {
    let mut target_msg = None;

    if
        ctx.message.image_message.as_option().is_some() ||
        ctx.message.video_message.as_option().is_some()
    {
        target_msg = Some(ctx.message.clone());
    } else if let Some(ext) = ctx.message.extended_text_message.as_option() {
        if let Some(ctx_info) = ext.context_info.as_option() {
            if let Some(quoted) = ctx_info.quoted_message.as_option() {
                if
                    quoted.image_message.as_option().is_some() ||
                    quoted.video_message.as_option().is_some()
                {
                    target_msg = Some(Arc::new(quoted.clone()));
                }
            }
        }
    }

    if let Some(msg) = target_msg {
        let sticker_result: anyhow::Result<(Vec<u8>, bool)> = if
            let Some(img) = msg.image_message.as_option()
        {
            match ctx.client.download(img).await {
                Ok(data) => {
                    log::info!("handle_s: downloaded image ({} bytes)", data.len());
                    run_blocking(move || convert_image_media_to_webp(&data), FFMPEG_TIMEOUT_SECS).await
                }
                Err(error) => {
                    log::error!("handle_s: image download failed: {error:?}");
                    Err(anyhow::anyhow!(error.to_string()))
                }
            }
        } else if let Some(vid) = msg.video_message.as_option() {
            match ctx.client.download(vid).await {
                Ok(data) => {
                    log::info!("handle_s: downloaded video ({} bytes)", data.len());
                    run_blocking(move || convert_video_media_to_webp(&data), STICKER_TO_VIDEO_TIMEOUT_SECS).await
                }
                Err(error) => {
                    log::error!("handle_s: video download failed: {error:?}");
                    Err(anyhow::anyhow!(error.to_string()))
                }
            }
        } else {
            return;
        };

        match sticker_result {
            Ok((webp_data, is_animated)) => {
                match
                    ctx.client.upload(webp_data, MediaType::Sticker, UploadOptions::default()).await
                {
                    Ok(upload) => {
                        let reply = wa::Message {
                            sticker_message: buffa::MessageField::some(wa::message::StickerMessage {
                                url: Some(upload.url),
                                direct_path: Some(upload.direct_path),
                                media_key: Some(upload.media_key.to_vec()),
                                file_sha256: Some(upload.file_sha256.to_vec()),
                                file_enc_sha256: Some(upload.file_enc_sha256.to_vec()),
                                file_length: Some(upload.file_length),
                                media_key_timestamp: Some(upload.media_key_timestamp),
                                mimetype: Some("image/webp".to_string()),
                                width: Some(STICKER_SIZE),
                                height: Some(STICKER_SIZE),
                                is_animated: Some(is_animated),
                                ..Default::default()
                            }),
                            ..Default::default()
                        };
                        let _ = ctx.send_message(reply).await;
                    }
                    Err(error) => {
                        log::error!("handle_s: sticker upload failed: {error:?}");
                        let _ = ctx.send_message(wa::Message {
                            conversation: Some("Failed to upload sticker.".to_string()),
                            ..Default::default()
                        }).await;
                    }
                }
            }
            Err(error) => {
                log::error!("handle_s: sticker conversion failed: {error:?}");
                let _ = ctx.send_message(wa::Message {
                    conversation: Some("Failed to convert media to sticker.".to_string()),
                    ..Default::default()
                }).await;
            }
        }
    } else {
        let _ = ctx.send_message(wa::Message {
            conversation: Some(
                "Please reply to an image, GIF, or video with 's' to convert it to a sticker.".to_string()
            ),
            ..Default::default()
        }).await;
    }
}

pub async fn handle_i(ctx: &MessageContext) {
    let mut target_msg = None;

    if ctx.message.sticker_message.as_option().is_some() {
        target_msg = Some(ctx.message.clone());
    } else if let Some(ext) = ctx.message.extended_text_message.as_option() {
        if let Some(ctx_info) = ext.context_info.as_option() {
            if let Some(quoted) = ctx_info.quoted_message.as_option() {
                if quoted.sticker_message.as_option().is_some() {
                    target_msg = Some(Arc::new(quoted.clone()));
                }
            }
        }
    }

    let Some(msg) = target_msg else {
        let _ = ctx.send_message(wa::Message {
            conversation: Some(
                "Please reply to a sticker with 'i' to convert it to an image.".to_string()
            ),
            ..Default::default()
        }).await;
        return;
    };

    let Some(sticker) = msg.sticker_message.as_option() else {
        return;
    };

    let data = match ctx.client.download(sticker).await {
        Ok(data) => {
            log::info!("handle_i: downloaded sticker ({} bytes)", data.len());
            data
        }
        Err(error) => {
            log::error!("handle_i: sticker download failed: {error:?}");
            let _ = ctx.send_message(wa::Message {
                conversation: Some("Failed to download sticker.".to_string()),
                ..Default::default()
            }).await;
            return;
        }
    };

    // Detect animation from the actual bitstream rather than trusting the
    // sender-provided is_animated flag, which isn't always set reliably.
    let is_animated = webp::BitstreamFeatures::new(&data)
        .map(|features| features.has_animation())
        .unwrap_or(false);

    if is_animated {
        log::info!("handle_i: detected animated sticker, converting to mp4");

        let video_result = run_blocking(move || animated_webp_to_mp4(&data), STICKER_TO_VIDEO_TIMEOUT_SECS).await;

        match video_result {
            Ok(mp4_data) => {
                match ctx.client.upload(mp4_data, MediaType::Video, UploadOptions::default()).await {
                    Ok(upload) => {
                        let reply = wa::Message {
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
                        let _ = ctx.send_message(reply).await;
                    }
                    Err(error) => {
                        log::error!("handle_i: video upload failed: {error:?}");
                        let _ = ctx.send_message(wa::Message {
                            conversation: Some("Failed to upload video.".to_string()),
                            ..Default::default()
                        }).await;
                    }
                }
            }
            Err(error) => {
                log::error!("handle_i: sticker->mp4 conversion failed: {error:?}");
                let error_text = error.to_string();
                let msg = if error_text.contains("too many frames") {
                    "This animated sticker is too long to convert to video. Try a shorter one."
                } else if error_text.contains("timed out") {
                    "Converting this sticker to video took too long. Try a shorter or simpler animated sticker."
                } else {
                    "Failed to convert animated sticker to video."
                };
                let _ = ctx.send_message(wa::Message {
                    conversation: Some(msg.to_string()),
                    ..Default::default()
                }).await;
            }
        }
        return;
    }

    // Static sticker path (unchanged behavior)
    match webp_to_image(&data) {
        Ok(img_data) => {
            match ctx.client.upload(img_data, MediaType::Image, UploadOptions::default()).await {
                Ok(upload) => {
                    let reply = wa::Message {
                        image_message: buffa::MessageField::some(wa::message::ImageMessage {
                            url: Some(upload.url),
                            direct_path: Some(upload.direct_path),
                            media_key: Some(upload.media_key.to_vec()),
                            file_sha256: Some(upload.file_sha256.to_vec()),
                            file_enc_sha256: Some(upload.file_enc_sha256.to_vec()),
                            file_length: Some(upload.file_length),
                            media_key_timestamp: Some(upload.media_key_timestamp),
                            mimetype: Some("image/jpeg".to_string()),
                            ..Default::default()
                        }),
                        ..Default::default()
                    };
                    let _ = ctx.send_message(reply).await;
                }
                Err(error) => {
                    log::error!("handle_i: image upload failed: {error:?}");
                    let _ = ctx.send_message(wa::Message {
                        conversation: Some("Failed to upload image.".to_string()),
                        ..Default::default()
                    }).await;
                }
            }
        }
        Err(error) => {
            log::error!("handle_i: sticker->image conversion failed: {error:?}");
            let _ = ctx.send_message(wa::Message {
                conversation: Some("Failed to convert sticker to image.".to_string()),
                ..Default::default()
            }).await;
        }
    }
}

// handle_r temporarily placed as public command

pub async fn handle_r(ctx: &MessageContext) {
    let mut target_msg = None;

    if let Some(ext) = ctx.message.extended_text_message.as_option() {
        if let Some(ctx_info) = ext.context_info.as_option() {
            if let Some(quoted) = ctx_info.quoted_message.as_option() {
                target_msg = Some(Arc::new(quoted.clone()));
            }
        }
    }

    if let Some(msg) = target_msg {
        if msg.is_view_once() {
            let base = msg.get_base_message();

            if let Some(img) = base.image_message.as_option() {
                let mut new_img = img.clone();
                new_img.view_once = Some(false);
                new_img.caption = None;
                let reply = wa::Message {
                    image_message: buffa::MessageField::some(new_img),
                    ..Default::default()
                };
                let _ = ctx.send_message(reply).await;
            } else if let Some(vid) = base.video_message.as_option() {
                let mut new_vid = vid.clone();
                new_vid.view_once = Some(false);
                let reply = wa::Message {
                    video_message: buffa::MessageField::some(new_vid),
                    ..Default::default()
                };
                let _ = ctx.send_message(reply).await;
            } else {
                let _ = ctx.send_message(wa::Message {
                    conversation: Some(
                        "The view-once message did not contain an image or video.".to_string()
                    ),
                    ..Default::default()
                }).await;
            }
        } else {
            let _ = ctx.send_message(wa::Message {
                conversation: Some(
                    "The replied message is not a view-once media message.".to_string()
                ),
                ..Default::default()
            }).await;
        }
    } else {
        let _ = ctx.send_message(wa::Message {
            conversation: Some(
                "Please reply to a view-once image or video with 'r' to resend it.".to_string()
            ),
            ..Default::default()
        }).await;
    }
}

fn find_youtube_url(text: &str) -> Option<String> {
    text.split_whitespace()
        .find(|word| word.contains("youtube.com/") || word.contains("youtu.be/"))
        .map(|s| s.trim().to_string())
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
            let _ = ctx.send_message(wa::Message {
                conversation: Some(
                    "Usage: 'd <YouTube URL>' or reply to a message containing a YouTube URL with 'd'.".to_string()
                ),
                ..Default::default()
            }).await;
            return;
        }
    };

    let _ = ctx.send_message(wa::Message {
        conversation: Some("Downloading video, please wait...".to_string()),
        ..Default::default()
    }).await;

    let download_result = tokio::time::timeout(
        Duration::from_secs(180),
        download_youtube_video(ytdlp, url.clone())
    ).await;

    match download_result {
        Ok(Ok(video_data)) => {
            log::info!("handle_d: downloaded {} bytes, uploading", video_data.len());
            match ctx.client.upload(video_data, MediaType::Video, UploadOptions::default()).await {
                Ok(upload) => {
                    let reply = wa::Message {
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
                    let _ = ctx.send_message(reply).await;
                }
                Err(e) => {
                    log::error!("handle_d: video upload failed: {e:?}");
                    let _ = ctx.send_message(wa::Message {
                        conversation: Some("Failed to upload video.".to_string()),
                        ..Default::default()
                    }).await;
                }
            }
        }
        Ok(Err(e)) => {
            log::error!("handle_d: download failed: {e:?}");
            let _ = ctx.send_message(wa::Message {
                conversation: Some(format!("Failed to download video: {e}")),
                ..Default::default()
            }).await;
        }
        Err(_) => {
            let _ = ctx.send_message(wa::Message {
                conversation: Some(
                    "Download timed out. The video may be too large or restricted.".to_string()
                ),
                ..Default::default()
            }).await;
        }
    }
}