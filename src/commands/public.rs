use std::io::Cursor;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use ffmpeg_sidecar::{ command::FfmpegCommand, download::auto_download };
use image::{ AnimationDecoder, GenericImageView, ImageDecoder, ImageFormat, Rgba, RgbaImage };
use tempfile::tempdir;
use whatsapp_rust::prelude::*;
use whatsapp_rust::upload::UploadOptions;
use whatsapp_rust::wacore::download::MediaType;

const STICKER_SIZE: u32 = 512;
const ANIMATED_STICKER_FPS: f32 = 15.0;
const FFMPEG_TIMEOUT_SECS: u64 = 60;

pub async fn handle_g(ctx: &MessageContext) {
    let chat = &ctx.info.source.chat;
    let _ = ctx.send_message(wa::Message {
        conversation: Some(chat.to_string()),
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

fn convert_video_to_webp(input_path: &Path, output_path: &Path) -> anyhow::Result<()> {
    if !ffmpeg_sidecar::command::ffmpeg_is_installed() {
        log::warn!("ffmpeg not installed, attempting auto_download()");
        auto_download().context("failed to download ffmpeg")?;
        log::info!("ffmpeg auto_download() succeeded");
    }

    let filtergraph = format!(
        "fps={fps},scale={size}:{size}:force_original_aspect_ratio=decrease,pad={size}:{size}:(ow-iw)/2:(oh-ih)/2:color=0x00000000",
        fps = ANIMATED_STICKER_FPS,
        size = STICKER_SIZE
    );
    let input = input_path.to_string_lossy().to_string();
    let output = output_path.to_string_lossy().to_string();

    log::info!("convert_video_to_webp: input={input} output={output}");

    let mut child = FfmpegCommand::new()
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

    // Drain ffmpeg's event stream and log it so stalls/errors are visible.
    for event in child.iter().context("failed to read ffmpeg output")? {
        match event {
            ffmpeg_sidecar::event::FfmpegEvent::Log(level, msg) => {
                log::debug!("ffmpeg[{level:?}]: {msg}");
            }
            ffmpeg_sidecar::event::FfmpegEvent::Error(err) => {
                log::error!("ffmpeg error event: {err}");
            }
            _ => {}
        }
    }

    let status = child.wait().context("failed to wait for ffmpeg")?;
    if !status.success() {
        anyhow::bail!("ffmpeg exited with status: {status}");
    }

    log::info!("convert_video_to_webp: success");
    Ok(())
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

/// Runs a blocking conversion closure on a dedicated thread pool with a timeout,
/// so slow/hanging ffmpeg or image work never stalls the async runtime.
async fn run_blocking_conversion<F>(f: F) -> anyhow::Result<(Vec<u8>, bool)>
    where F: FnOnce() -> anyhow::Result<(Vec<u8>, bool)> + Send + 'static
{
    let join_result = tokio::time::timeout(
        Duration::from_secs(FFMPEG_TIMEOUT_SECS),
        tokio::task::spawn_blocking(f)
    ).await;

    match join_result {
        Ok(Ok(inner)) => inner,
        Ok(Err(join_err)) => Err(anyhow::anyhow!("conversion task panicked: {join_err}")),
        Err(_) => Err(anyhow::anyhow!("conversion timed out after {FFMPEG_TIMEOUT_SECS}s")),
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
                    run_blocking_conversion(move || convert_image_media_to_webp(&data)).await
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
                    run_blocking_conversion(move || convert_video_media_to_webp(&data)).await
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

    if let Some(msg) = target_msg {
        if let Some(sticker) = msg.sticker_message.as_option() {
            let downloaded: Result<Vec<u8>, _> = ctx.client.download(sticker).await;
            match downloaded {
                Ok(data) => {
                    match webp_to_image(&data) {
                        Ok(img_data) => {
                            match
                                ctx.client.upload(
                                    img_data,
                                    MediaType::Image,
                                    UploadOptions::default()
                                ).await
                            {
                                Ok(upload) => {
                                    let reply = wa::Message {
                                        image_message: buffa::MessageField::some(
                                            wa::message::ImageMessage {
                                                url: Some(upload.url),
                                                direct_path: Some(upload.direct_path),
                                                media_key: Some(upload.media_key.to_vec()),
                                                file_sha256: Some(upload.file_sha256.to_vec()),
                                                file_enc_sha256: Some(
                                                    upload.file_enc_sha256.to_vec()
                                                ),
                                                file_length: Some(upload.file_length),
                                                media_key_timestamp: Some(
                                                    upload.media_key_timestamp
                                                ),
                                                mimetype: Some("image/jpeg".to_string()),
                                                ..Default::default()
                                            }
                                        ),
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
                                conversation: Some(
                                    "Failed to convert sticker to image.".to_string()
                                ),
                                ..Default::default()
                            }).await;
                        }
                    }
                }
                Err(error) => {
                    log::error!("handle_i: sticker download failed: {error:?}");
                    let _ = ctx.send_message(wa::Message {
                        conversation: Some("Failed to download sticker.".to_string()),
                        ..Default::default()
                    }).await;
                }
            }
        }
    } else {
        let _ = ctx.send_message(wa::Message {
            conversation: Some(
                "Please reply to a sticker with 'i' to convert it to an image.".to_string()
            ),
            ..Default::default()
        }).await;
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
                new_img.caption = None; // optionally strip original caption, or keep it.
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