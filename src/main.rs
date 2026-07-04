mod config;
mod commands;
mod handlers;

use config::Config;
use log::{error, info};
use std::sync::Arc;
use whatsapp_rust::prelude::*;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let config = Arc::new(Config::load());
    info!("Configuration loaded");

    // Persist session to local directory "creds"
    let _ = std::fs::create_dir_all("creds");
    let store = SqliteStore::new("creds/whatsapp.db").await?;
    info!("SQLite backend initialized");

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
            move |ctx| {
                let config = config.clone();
                async move {
                    handlers::handle_message(ctx, config).await;
                }
            }
        })
        .build()
        .await?;

    info!("Starting bot...");
    bot.run().await;
    Ok(())
}
