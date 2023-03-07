use anyhow::Result;
use bot::Bot;
use dotenv::dotenv;
use grammers_client::{Client, Config};
use grammers_session::Session;
use log::info;
use simplelog::TermLogger;

mod bot;
mod command;

#[tokio::main]
async fn main() -> Result<()> {
    // Load .env file
    dotenv().ok();

    // Initialize logging
    TermLogger::init(
        log::LevelFilter::Info,
        simplelog::Config::default(),
        simplelog::TerminalMode::Mixed,
        simplelog::ColorChoice::Auto,
    )?;

    // Load environment variables
    let api_id = std::env::var("API_ID")?.parse()?;
    let api_hash = std::env::var("API_HASH")?;
    let bot_token = std::env::var("BOT_TOKEN")?;

    // Fill in the configuration and connect to Telegram
    let config = Config {
        api_id,
        api_hash: api_hash.clone(),
        session: Session::load_file_or_create("session.bin")?,
        params: Default::default(),
    };
    let client = Client::connect(config).await?;

    // Authorize as a bot if needed
    if !client.is_authorized().await? {
        info!("Not authorized, signing in");
        client.bot_sign_in(&bot_token, api_id, &api_hash).await?;
    }

    // Save the session to a file
    client.session().save_to_file("session.bin")?;

    // Create the bot and run it
    let bot = Bot::new(client).await?;
    bot.run().await?;

    Ok(())
}
