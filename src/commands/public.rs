use std::sync::Arc;
use whatsapp_rust::prelude::*;
use whatsapp_rust::wacore::download::MediaType;
use whatsapp_rust::upload::UploadOptions;

pub async fn handle_groupid(ctx: &MessageContext) {
    let chat = &ctx.info.source.chat;
    let _ = ctx.send_message(wa::Message {
        conversation: Some(chat.to_string()),
        ..Default::default()
    }).await;
}

pub fn image_to_webp(data: &[u8]) -> anyhow::Result<Vec<u8>> {
    let img = image::load_from_memory(data)?;
    let img = img.resize(512, 512, image::imageops::FilterType::Lanczos3);
    
    let encoder = webp::Encoder::from_image(&img)
        .map_err(|e| anyhow::anyhow!("WebP encoding error: {:?}", e))?;
        
    let webp = encoder.encode(80.0);
    Ok(webp.to_vec())
}

pub fn webp_to_image(data: &[u8]) -> anyhow::Result<Vec<u8>> {
    let img = image::load_from_memory(data)?;
    let mut buf = std::io::Cursor::new(Vec::new());
    img.write_to(&mut buf, image::ImageFormat::Jpeg)?;
    Ok(buf.into_inner())
}

pub async fn handle_s(ctx: &MessageContext) {
    let mut target_msg = None;

    if ctx.message.image_message.as_option().is_some() || ctx.message.video_message.as_option().is_some() {
        target_msg = Some(ctx.message.clone());
    } else if let Some(ext) = ctx.message.extended_text_message.as_option() {
        if let Some(ctx_info) = ext.context_info.as_option() {
            if let Some(quoted) = ctx_info.quoted_message.as_option() {
                if quoted.image_message.as_option().is_some() || quoted.video_message.as_option().is_some() {
                    target_msg = Some(Arc::new(quoted.clone()));
                }
            }
        }
    }

    if let Some(msg) = target_msg {
        let downloaded: Result<Vec<u8>, _> = if let Some(img) = msg.image_message.as_option() {
            ctx.client.download(img).await
        } else if let Some(vid) = msg.video_message.as_option() {
            if vid.gif_playback.unwrap_or(false) {
                // Note: Pure rust webp encoding for animated GIFs is currently complex. 
                // We will decode the first frame for now as per the pure-rust approval.
                ctx.client.download(vid).await
            } else {
                let _ = ctx.send_message(wa::Message {
                    conversation: Some("I can only process images or GIFs. Other video formats are not supported.".to_string()),
                    ..Default::default()
                }).await;
                return;
            }
        } else {
            return;
        };

        match downloaded {
            Ok(data) => {
                match image_to_webp(&data) {
                    Ok(webp_data) => {
                        if let Ok(upload) = ctx.client.upload(webp_data, MediaType::Sticker, UploadOptions::default()).await {
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
                                    ..Default::default()
                                }),
                                ..Default::default()
                            };
                            let _ = ctx.send_message(reply).await;
                        } else {
                            let _ = ctx.send_message(wa::Message {
                                conversation: Some("Failed to upload sticker.".to_string()),
                                ..Default::default()
                            }).await;
                        }
                    },
                    Err(_) => {
                        let _ = ctx.send_message(wa::Message {
                            conversation: Some("Failed to convert image to sticker. Ensure it's a valid image.".to_string()),
                            ..Default::default()
                        }).await;
                    }
                }
            }
            Err(_) => {
                let _ = ctx.send_message(wa::Message {
                    conversation: Some("Failed to download media.".to_string()),
                    ..Default::default()
                }).await;
            }
        }
    } else {
        let _ = ctx.send_message(wa::Message {
            conversation: Some("Please reply to an image or GIF with 's' to convert it to a sticker.".to_string()),
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
                            if let Ok(upload) = ctx.client.upload(img_data, MediaType::Image, UploadOptions::default()).await {
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
                            } else {
                                let _ = ctx.send_message(wa::Message {
                                    conversation: Some("Failed to upload image.".to_string()),
                                    ..Default::default()
                                }).await;
                            }
                        },
                        Err(_) => {
                            let _ = ctx.send_message(wa::Message {
                                conversation: Some("Failed to convert sticker to image.".to_string()),
                                ..Default::default()
                            }).await;
                        }
                    }
                }
                Err(_) => {
                    let _ = ctx.send_message(wa::Message {
                        conversation: Some("Failed to download sticker.".to_string()),
                        ..Default::default()
                    }).await;
                }
            }
        }
    } else {
        let _ = ctx.send_message(wa::Message {
            conversation: Some("Please reply to a sticker with 'i' to convert it to an image.".to_string()),
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
                new_img.caption = None; // optionally strip original caption, or keep it. Let's keep it but maybe append a note? Or just leave it as is.
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
                    conversation: Some("The view-once message did not contain an image or video.".to_string()),
                    ..Default::default()
                }).await;
            }
        } else {
            let _ = ctx.send_message(wa::Message {
                conversation: Some("The replied message is not a view-once media message.".to_string()),
                ..Default::default()
            }).await;
        }
    } else {
        let _ = ctx.send_message(wa::Message {
            conversation: Some("Please reply to a view-once image or video with 'r' to resend it.".to_string()),
            ..Default::default()
        }).await;
    }
}