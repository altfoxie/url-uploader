use std::{sync::Arc, time::Duration};

use anyhow::Result;
use async_read_progress::TokioAsyncReadProgressExt;
use futures::TryStreamExt;
use grammers_client::{
    types::{Chat, Message, User},
    Client, InputMessage, Update,
};
use log::{error, info, warn};
use reqwest::Url;
use tokio::sync::Mutex;
use tokio_util::compat::FuturesAsyncReadCompatExt;

use crate::command::{parse_command, Command};

/// Bot is the main struct of the bot.
/// All the bot logic is implemented in this struct.
#[derive(Debug)]
pub struct Bot {
    client: Client,
    me: User,
    http: reqwest::Client,
}

impl Bot {
    /// Create a new bot instance.
    pub async fn new(client: Client) -> Result<Arc<Self>> {
        let me = client.get_me().await?;
        let http = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/110.0.0.0 Safari/537.36")
            .build()?;
        Ok(Arc::new(Self { client, me, http }))
    }

    /// Run the bot.
    pub async fn run(self: Arc<Self>) -> Result<()> {
        while let Some(update) = tokio::select! {
            _ = tokio::signal::ctrl_c() => {
                info!("Received Ctrl+C, exiting");
                Ok(None)
            }
            update = self.client.next_update() => update
        }? {
            let self_ = self.clone();

            // Spawn a new task to handle the update
            tokio::spawn(async move {
                if let Err(err) = self_.handle_update(update).await {
                    error!("Error handling update: {}", err);
                }
            });
        }

        Ok(())
    }

    /// Update handler.
    async fn handle_update(&self, update: Update) -> Result<()> {
        // NOTE: no ; here, so result is returned
        match update {
            Update::NewMessage(msg) => self.handle_message(msg).await,
            _ => Ok(()),
        }
    }

    /// Message handler.
    ///
    /// Ensures the message is from a user or a group, and then parses the command.
    /// If the command is not recognized, it will try to parse the message as a URL.
    async fn handle_message(&self, msg: Message) -> Result<()> {
        // Ensure the message chat is a user or a group
        match msg.chat() {
            Chat::User(_) | Chat::Group(_) => {}
            _ => return Ok(()),
        };

        // Parse the command
        let command = parse_command(msg.text());
        if let Some(command) = command {
            // Ensure the command is for this bot
            if let Some(via) = &command.via {
                if via.to_lowercase() != self.me.username().unwrap_or_default().to_lowercase() {
                    warn!("Ignoring command for unknown bot: {}", via);
                    return Ok(());
                }
            }

            // There is a chance that there are multiple bots listening
            // to /start commands in a group, so we handle commands
            // only if they are sent explicitly to this bot.
            if let Chat::Group(_) = msg.chat() {
                if command.name == "start" && command.via.is_none() {
                    return Ok(());
                }
            }

            // Handle the command
            info!("Received command: {:?}", command);
            match command.name.as_str() {
                "start" => {
                    return self.handle_start(msg).await;
                }
                "upload" => {
                    return self.handle_upload(msg, command).await;
                }
                _ => {}
            }
        }

        // If the message is not a command, try to parse it as a URL
        if let Ok(url) = Url::parse(msg.text()) {
            return self.handle_url(msg, url).await;
        }

        Ok(())
    }

    /// Handle the /start command.
    /// This command is sent when the user starts a conversation with the bot.
    /// It will reply with a welcome message.
    async fn handle_start(&self, msg: Message) -> Result<()> {
        msg.reply(InputMessage::html(
            "📁 <b>Hi! Drop me a link to a file and I'll upload it for you.</b>\n\
            <i>In groups you can use the command /upload &lt;url&gt;.</i>\n\
            \n\
            🌟 <b>Features:</b>\n\
            \u{2022} <a href=\"https://github.com/altfoxie/url-uploader\">Open source</a>\n\
            \u{2022} Free & fast\n\
            \u{2022} Uploads files up to 2GB\n\
            \u{2022} Supports redirects",
        ))
        .await?;
        Ok(())
    }

    /// Handle the /upload command.
    /// This command should be used in groups to upload a file.
    async fn handle_upload(&self, msg: Message, cmd: Command) -> Result<()> {
        // If the argument is not specified, reply with an error
        let url = match cmd.arg {
            Some(url) => url,
            None => {
                msg.reply("Please specify a URL").await?;
                return Ok(());
            }
        };

        // Parse the URL
        let url = match Url::parse(&url) {
            Ok(url) => url,
            Err(err) => {
                msg.reply(format!("Invalid URL: {}", err)).await?;
                return Ok(());
            }
        };

        self.handle_url(msg, url).await
    }

    /// Handle a URL.
    /// This function will download the file and upload it to Telegram.
    async fn handle_url(&self, msg: Message, url: Url) -> Result<()> {
        info!("Downloading file from {}", url);
        let response = self.http.get(url).send().await?;

        // Get the file name and size
        let length = response.content_length().unwrap_or_default() as usize;
        let name = match response
            .headers()
            .get("content-disposition")
            .and_then(|value| {
                value
                    .to_str()
                    .ok()
                    .and_then(|value| {
                        value
                            .split(';')
                            .map(|value| value.trim())
                            .find(|value| value.starts_with("filename="))
                    })
                    .map(|value| value.trim_start_matches("filename="))
                    .map(|value| value.trim_matches('"'))
            }) {
            Some(name) => name.to_string(),
            None => response
                .url()
                .path_segments()
                .and_then(|segments| segments.last())
                .unwrap_or("file.bin")
                .to_string(),
        };
        info!("File {} ({} bytes)", name, length);

        // File is empty
        if length == 0 {
            msg.reply("File is empty").await?;
            return Ok(());
        }

        // File is too large
        if length > 2 * 1024 * 1024 * 1024 {
            msg.reply("File is too large").await?;
            return Ok(());
        }

        // Send status message
        let status = Arc::new(Mutex::new(
            msg.reply(InputMessage::html(format!(
                "Starting upload of <code>{}</code>...",
                name
            )))
            .await?,
        ));

        let mut stream = response
            .bytes_stream()
            // TODO: idk why this is needed
            .map_err(|err| std::io::Error::new(std::io::ErrorKind::Other, err))
            .into_async_read()
            .compat()
            // Report progress every 3 seconds
            .report_progress(Duration::from_secs(3), |progress| {
                let status = status.clone();
                let name = name.clone();
                tokio::spawn(async move {
                    status
                        .lock()
                        .await
                        .edit(InputMessage::html(format!(
                            "Uploading <code>{}</code> ({:.2}%)\n - {} / {}",
                            name,
                            progress as f64 / length as f64 * 100.0,
                            bytesize::to_string(progress as u64, true),
                            bytesize::to_string(length as u64, true),
                        )))
                        .await
                        .ok();
                });
            });
        let file = self
            .client
            .upload_stream(&mut stream, length, name.clone())
            .await?;

        // Send file
        msg.reply(InputMessage::default().file(file)).await?;

        // Delete status message
        status.lock().await.delete().await?;

        Ok(())
    }
}
