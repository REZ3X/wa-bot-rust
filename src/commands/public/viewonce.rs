use std::sync::Arc;
use whatsapp_rust::prelude::*;
use whatsapp_rust::wacore::proto_helpers::MessageExt;

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
                let mut reply = wa::Message {
                    image_message: buffa::MessageField::some(new_img),
                    ..Default::default()
                };
                reply.set_context_info(ctx.build_quote_context());
                let _ = ctx.send_message(reply).await;
            } else if let Some(vid) = base.video_message.as_option() {
                let mut new_vid = vid.clone();
                new_vid.view_once = Some(false);
                let mut reply = wa::Message {
                    video_message: buffa::MessageField::some(new_vid),
                    ..Default::default()
                };
                reply.set_context_info(ctx.build_quote_context());
                let _ = ctx.send_message(reply).await;
            } else {
                let _ = ctx.reply_quoting(
                    "The view-once message did not contain an image or video."
                ).await;
            }
        } else {
            let _ = ctx.reply_quoting(
                "The replied message is not a view-once media message."
            ).await;
        }
    } else {
        let _ = ctx.reply_quoting(
            "Please reply to a view-once image or video with 'r' to resend it."
        ).await;
    }
}
