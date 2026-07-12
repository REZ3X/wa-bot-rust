use crate::commands;
use crate::commands::public::YtDlpContext;
use crate::config::Config;
use std::sync::Arc;
use whatsapp_rust::prelude::*;
use whatsapp_rust::wacore::proto_helpers::MessageExt;
use whatsapp_rust::wacore_binary::JidExt;

pub async fn handle_message(ctx: MessageContext, config: Arc<Config>, ytdlp_ctx: Arc<YtDlpContext>) {
    let chat = &ctx.info.source.chat;
    let sender = &ctx.info.source.sender;
    let is_from_me = ctx.info.source.is_from_me;

    if is_from_me {
        return;
    }

    let text_content = ctx.message
        .text_content()
        .or_else(|| ctx.message.get_caption())
        .unwrap_or_default()
        .trim()
        .to_string();

    if text_content.is_empty() {
        return;
    }

    let is_group = chat.is_group();
    let is_allowed_group = is_group && config.is_group_allowed(&chat.to_string());

    // Only whitelisted groups are allowed. The bot is completely silent in
    // private DMs and non-whitelisted groups — no response, no indication
    // it exists.
    if !is_allowed_group {
        return;
    }

    if text_content == "c" {
        log::info!("dispatch: 'c' command from {sender}");
        commands::public::handle_c(&ctx).await;
        return;
    }

    if text_content == "h" {
        log::info!("dispatch: 'h' command from {sender}");
        commands::public::handle_h(&ctx).await;
        return;
    }

    if text_content == "t" {
        log::info!("dispatch: 't' command from {sender}");
        commands::public::handle_t(&ctx).await;
        return;
    }

    if text_content == "s" {
        log::info!("dispatch: 's' command from {sender}");
        commands::public::handle_s(&ctx).await;
        return;
    }

    if text_content == "i" {
        log::info!("dispatch: 'i' command from {sender}");
        commands::public::handle_i(&ctx).await;
        return;
    }

    if text_content == "r" {
        log::info!("dispatch: 'r' command from {sender}");
        commands::public::handle_r(&ctx).await;
        return;
    }

    if text_content == "d" || text_content.starts_with("d ") {
        log::info!("dispatch: 'd' command from {sender}");
        commands::public::handle_d(&ctx, &ytdlp_ctx).await;
        return;
    }

    // Admin privileged for 'r' command is disabled until the phone numbers identifier is fixed

    // if text_content == "r" {
    //     if config.is_admin(&sender.to_string()) {
    //         commands::admin::handle_r(&ctx).await;
    //     } else {
    //         let _ = ctx
    //             .send_message(wa::Message {
    //                 conversation: Some(
    //                     "You do not have permission to use this command.".to_string(),
    //                 ),
    //                 ..Default::default()
    //             })
    //             .await;
    //     }
    //     return;
    // }
}