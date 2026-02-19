use super::client;
use super::config::PollerConfig;
use super::types::PollEvent;

pub fn poll_watcher(config: PollerConfig) -> impl futures::Stream<Item = PollEvent> {
    iced::stream::channel(10, |mut output| async move {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(15))
            .build()
            .unwrap();

        use futures::SinkExt;

        // Signal connected
        let _ = output.send(PollEvent::Connected).await;

        loop {
            tokio::time::sleep(std::time::Duration::from_secs(config.poll_interval_secs)).await;

            match client::fetch_pending(&client, &config).await {
                Ok(messages) => {
                    if !messages.is_empty() {
                        tracing::info!("Polled {} pending message(s)", messages.len());
                        if output
                            .send(PollEvent::MessagesReceived(messages))
                            .await
                            .is_err()
                        {
                            break;
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!("Poll error: {e}");
                    if output.send(PollEvent::Error(e)).await.is_err() {
                        break;
                    }
                }
            }
        }
    })
}
