use super::handler;

#[derive(Debug, Clone)]
pub enum UploadEvent {
    Started(String),
    PhotoReceived(Vec<u8>),
    Error(String),
}

pub fn upload_server(bind_addr: String) -> impl futures::Stream<Item = UploadEvent> {
    iced::stream::channel(10, |mut output| async move {
        use futures::SinkExt;

        let (tx, mut rx) = tokio::sync::mpsc::channel::<Vec<u8>>(16);
        let router = handler::build_router(tx);

        let listener = match tokio::net::TcpListener::bind(&bind_addr).await {
            Ok(l) => l,
            Err(e) => {
                let _ = output
                    .send(UploadEvent::Error(format!("Bind {bind_addr}: {e}")))
                    .await;
                return;
            }
        };

        tracing::info!("Upload server listening on {bind_addr}");
        let _ = output.send(UploadEvent::Started(bind_addr)).await;

        tokio::spawn(async move {
            if let Err(e) = axum::serve(listener, router).await {
                tracing::error!("Upload server died: {e}");
            }
        });

        while let Some(image_bytes) = rx.recv().await {
            if output.send(UploadEvent::PhotoReceived(image_bytes)).await.is_err() {
                break;
            }
        }
    })
}
