use super::*;
use std::net::IpAddr;
use std::sync::Weak;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

pub(super) struct Registry {
    all: Arc<Semaphore>,
    devices: std::sync::Mutex<HashMap<String, Weak<Semaphore>>>,
}

struct Slots {
    _all: OwnedSemaphorePermit,
    _device: OwnedSemaphorePermit,
}

impl Registry {
    pub(super) fn new() -> Self {
        Self {
            all: Arc::new(Semaphore::new(32)),
            devices: std::sync::Mutex::new(HashMap::new()),
        }
    }

    fn acquire(&self, device: &str) -> Result<Slots, ApiError> {
        let mut devices = self.devices.lock().unwrap_or_else(|e| e.into_inner());
        devices.retain(|_, value| value.strong_count() > 0);
        let limit = devices
            .get(device)
            .and_then(Weak::upgrade)
            .unwrap_or_else(|| {
                let limit = Arc::new(Semaphore::new(8));
                devices.insert(device.to_string(), Arc::downgrade(&limit));
                limit
            });
        Ok(Slots {
            _device: limit
                .try_acquire_owned()
                .map_err(|_| ApiError::new(429, "浏览器连接数已满"))?,
            _all: self
                .all
                .clone()
                .try_acquire_owned()
                .map_err(|_| ApiError::new(429, "浏览器连接数已满"))?,
        })
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Target {
    host: String,
    port: u16,
}

impl Target {
    fn parse(text: &str) -> Result<Self, ApiError> {
        let invalid = || ApiError::new(400, "浏览器目标地址无效");
        if text.len() > 1024 {
            return Err(invalid());
        }
        let mut target: Self = serde_json::from_str(text).map_err(|_| invalid())?;
        target.host = target.host.trim_end_matches('.').to_ascii_lowercase();
        if target.port == 0 || target.host.is_empty() || target.host.len() > 253 {
            return Err(invalid());
        }
        if let Ok(address) = target.host.parse::<IpAddr>() {
            let address = match address {
                IpAddr::V6(address) => address
                    .to_ipv4_mapped()
                    .map(IpAddr::V4)
                    .unwrap_or(IpAddr::V6(address)),
                address => address,
            };
            if address.is_multicast()
                || address.is_unspecified()
                || address.to_string() == "127.0.0.2"
            {
                return Err(invalid());
            }
            target.host = address.to_string();
        } else if !target.host.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || c == b'-')
        }) {
            return Err(invalid());
        }
        if target.host == "localhost" || target.host.ends_with(".localhost") {
            target.host = "127.0.0.1".into();
        }
        Ok(target)
    }
}

pub(super) async fn tunnel(
    State(state): State<WebState>,
    headers: HeaderMap,
    upgrade: WebSocketUpgrade,
) -> Result<Response, ApiError> {
    let device = authorized(&state, &headers).await?;
    let authorization = state
        .controller
        .authorization(device.clone(), Some("browserTunnel"));
    authorization.check().await.map_err(authorization_error)?;
    let slots = state.controller.browser_tunnels.acquire(&device.id)?;
    Ok(upgrade
        .max_message_size(65536)
        .max_frame_size(65536)
        .on_upgrade(move |mut socket| async move {
            let _slots = slots;
            // Dropping either branch closes pending DNS, TCP, and backpressured writes.
            let result = tokio::select! {
                result = transfer(&mut socket, &authorization) => result,
                error = authorization_lost(&authorization) => Err(error),
            };
            if let Err(error) = result {
                let response = json!({"ok":false,"code":error.code,"message":error.message});
                let _ = tokio::time::timeout(
                    Duration::from_secs(2),
                    socket.send(WebSocketMessage::Text(response.to_string().into())),
                )
                .await;
            }
            let _ = tokio::time::timeout(Duration::from_secs(2), socket.close()).await;
        })
        .into_response())
}

async fn authorization_lost(authorization: &RemoteAuthorization) -> ApiError {
    let mut events = authorization.controller.events.subscribe();
    let mut interval = tokio::time::interval(Duration::from_secs(1));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            _ = interval.tick() => {},
            _ = events.recv() => {},
        }
        if let Err(error) = authorization.check().await {
            return authorization_error(error);
        }
    }
}

async fn transfer(
    socket: &mut WebSocket,
    authorization: &RemoteAuthorization,
) -> Result<(), ApiError> {
    let first = tokio::time::timeout(Duration::from_secs(10), socket.recv())
        .await
        .map_err(|_| ApiError::new(408, "浏览器连接请求超时"))?
        .ok_or_else(|| ApiError::new(400, "缺少浏览器目标地址"))?
        .map_err(|_| ApiError::new(400, "浏览器连接已关闭"))?;
    let WebSocketMessage::Text(text) = first else {
        return Err(ApiError::new(400, "浏览器连接首帧必须为目标地址"));
    };
    let target = Target::parse(&text)?;
    authorization.check().await.map_err(authorization_error)?;
    let mut tcp = tokio::time::timeout(
        Duration::from_secs(15),
        TcpStream::connect((target.host.as_str(), target.port)),
    )
    .await
    .map_err(|_| ApiError::new(504, "电脑连接目标地址超时"))?
    .map_err(|_| ApiError::new(502, "电脑无法连接目标地址"))?;
    let _ = tcp.set_nodelay(true);
    authorization.check().await.map_err(authorization_error)?;
    send(socket, WebSocketMessage::Text(r#"{"ok":true}"#.into())).await?;
    use futures_util::StreamExt;
    let (mut outgoing, mut incoming) = socket.split();
    let (sender, mut receiver) = tokio::sync::mpsc::channel::<WebSocketMessage>(32);
    let (mut tcp_read, mut tcp_write) = tcp.split();
    // Independent pumps are necessary when both peers write more than a TCP window.
    let download = async {
        let mut buffer = [0u8; 32768];
        loop {
            let count = tcp_read
                .read(&mut buffer)
                .await
                .map_err(|_| ApiError::new(502, "目标连接中断"))?;
            if count == 0 {
                return Ok(());
            }
            tokio::time::timeout(
                Duration::from_secs(30),
                sender.send(WebSocketMessage::Binary(buffer[..count].to_vec().into())),
            )
            .await
            .map_err(|_| ApiError::new(504, "浏览器接收超时"))?
            .map_err(|_| ApiError::new(502, "浏览器连接中断"))?;
        }
    };
    let upload = async {
        let mut write_closed = false;
        loop {
            match incoming.next().await {
                None | Some(Ok(WebSocketMessage::Close(_))) => return Ok(()),
                Some(Ok(WebSocketMessage::Binary(bytes))) if !write_closed => {
                    tokio::time::timeout(Duration::from_secs(30), tcp_write.write_all(&bytes))
                        .await
                        .map_err(|_| ApiError::new(504, "目标写入超时"))?
                        .map_err(|_| ApiError::new(502, "目标连接中断"))?;
                }
                Some(Ok(WebSocketMessage::Text(text)))
                    if !write_closed
                        && serde_json::from_str::<Value>(&text).ok()
                            == Some(json!({"eof":true})) =>
                {
                    tcp_write
                        .shutdown()
                        .await
                        .map_err(|_| ApiError::new(502, "目标连接中断"))?;
                    write_closed = true;
                }
                Some(Ok(WebSocketMessage::Ping(bytes))) => {
                    tokio::time::timeout(
                        Duration::from_secs(30),
                        sender.send(WebSocketMessage::Pong(bytes)),
                    )
                    .await
                    .map_err(|_| ApiError::new(504, "浏览器接收超时"))?
                    .map_err(|_| ApiError::new(502, "浏览器连接中断"))?;
                }
                Some(Ok(WebSocketMessage::Pong(_))) => {}
                _ => return Err(ApiError::new(400, "浏览器数据帧无效")),
            }
        }
    };
    let write = async {
        while let Some(message) = receiver.recv().await {
            outgoing
                .send(message)
                .await
                .map_err(|_| ApiError::new(502, "浏览器连接中断"))?;
        }
        Ok(())
    };
    tokio::select! {
        result = upload => result,
        result = download => result,
        result = write => result,
    }
}

async fn send(socket: &mut WebSocket, message: WebSocketMessage) -> Result<(), ApiError> {
    tokio::time::timeout(Duration::from_secs(30), socket.send(message))
        .await
        .map_err(|_| ApiError::new(504, "浏览器接收超时"))?
        .map_err(|_| ApiError::new(502, "浏览器连接中断"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::StreamExt;
    use tokio_tungstenite::tungstenite::Message;
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;

    struct NoNative;
    #[async_trait]
    impl NativeRemoteBridge for NoNative {
        async fn call(&self, _: &str, _: Value) -> anyhow::Result<Value> {
            anyhow::bail!("browser tunnel must not call Codex");
        }
    }

    async fn fixture() -> (
        tempfile::TempDir,
        Arc<RemoteAccessController>,
        String,
        tokio::task::JoinHandle<()>,
    ) {
        let directory = tempfile::tempdir().unwrap();
        configure_access_key(directory.path(), "browser-fixture-key").unwrap();
        let controller =
            RemoteAccessController::new(Arc::new(NoNative), directory.path().to_path_buf())
                .unwrap();
        controller.publish_access_policy(&RemoteAccessSettings {
            enabled: true,
            read_only: false,
            ..RemoteAccessSettings::default()
        });
        controller.running.store(true, Ordering::SeqCst);
        {
            let mut auth = controller.auth.lock().await;
            let key_id = auth.access_key.as_ref().unwrap().id.clone();
            auth.devices.push(Device {
                id: "browser-device".into(),
                name: "browser fixture".into(),
                token_hash: hash("browser-cookie"),
                created_at_ms: now_ms(),
                expires_at_ms: now_ms() + 120_000,
                last_seen_at_ms: now_ms(),
                key_id,
                public_origin: None,
            });
        }
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("ws://{}/api/browser/tunnel", listener.local_addr().unwrap());
        let router = Router::new()
            .route("/api/browser/tunnel", get(tunnel))
            .with_state(WebState {
                controller: controller.clone(),
                settings: RemoteAccessSettings::default(),
            });
        let server = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        (directory, controller, url, server)
    }

    fn request(
        url: &str,
        authenticated: bool,
    ) -> tokio_tungstenite::tungstenite::http::Request<()> {
        let mut request = url.into_client_request().unwrap();
        if authenticated {
            request.headers_mut().insert(
                "cookie",
                "codex_elves_device=browser-cookie".parse().unwrap(),
            );
        }
        request
    }

    #[tokio::test]
    async fn rejects_anonymous_and_read_only_before_upgrade() {
        let (_directory, controller, url, server) = fixture().await;
        for (authenticated, expected) in [(false, 401), (true, 403)] {
            if authenticated {
                controller.publish_access_policy(&RemoteAccessSettings {
                    enabled: true,
                    read_only: true,
                    ..RemoteAccessSettings::default()
                });
            }
            let error = tokio_tungstenite::connect_async(request(&url, authenticated))
                .await
                .unwrap_err();
            match error {
                tokio_tungstenite::tungstenite::Error::Http(response) => {
                    assert_eq!(response.status().as_u16(), expected)
                }
                other => panic!("{other:?}"),
            }
        }
        server.abort();
    }

    #[test]
    fn validates_targets_without_resolving_dns() {
        for target in [
            json!({"host":"http://localhost","port":80}),
            json!({"host":"user@localhost","port":80}),
            json!({"host":"127.0.0.2","port":80}),
            json!({"host":"::ffff:127.0.0.2","port":80}),
            json!({"host":"0.0.0.0","port":80}),
            json!({"host":"224.0.0.1","port":80}),
            json!({"host":"localhost","port":0}),
            json!({"host":"localhost","port":65536}),
            json!({"host":"a..test","port":80}),
            json!({"host":"localhost","port":80,"cookie":"bad"}),
        ] {
            assert!(Target::parse(&target.to_string()).is_err(), "{target}");
        }
        assert_eq!(
            Target::parse(r#"{"host":"app.localhost","port":4399}"#)
                .unwrap()
                .host,
            "127.0.0.1"
        );
        assert_eq!(
            Target::parse(r#"{"host":"service.example.test","port":443}"#)
                .unwrap()
                .port,
            443
        );
        assert_eq!(
            Target::parse(r#"{"host":"::1","port":80}"#).unwrap().host,
            "::1"
        );
    }

    #[test]
    fn connection_limits_are_per_device_and_global_and_released() {
        let registry = Registry::new();
        let mut slots = Vec::new();
        for device in ["a", "b", "c", "d"] {
            for _ in 0..8 {
                slots.push(registry.acquire(device).unwrap());
            }
            assert!(registry.acquire(device).is_err());
        }
        assert!(registry.acquire("e").is_err());
        slots.pop();
        assert!(registry.acquire("e").is_ok());
        drop(slots);
        assert_eq!(registry.all.available_permits(), 32);
    }

    #[tokio::test]
    async fn transfers_large_full_duplex_binary_streams_and_half_close() {
        let (_directory, _controller, url, server) = fixture().await;
        let target = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = target.local_addr().unwrap().port();
        let upload = (0..2 * 1024 * 1024)
            .map(|i| (i % 251) as u8)
            .collect::<Vec<_>>();
        let download = (0..3 * 1024 * 1024)
            .map(|i| (i % 193) as u8)
            .collect::<Vec<_>>();
        let expected_upload = upload.clone();
        let expected_download = download.clone();
        let peer = tokio::spawn(async move {
            let (mut connection, _) = target.accept().await.unwrap();
            let (mut reader, mut writer) = connection.split();
            let receive = async {
                let mut bytes = Vec::new();
                reader.read_to_end(&mut bytes).await.unwrap();
                assert_eq!(bytes, expected_upload);
            };
            let transmit = async {
                writer.write_all(&download).await.unwrap();
            };
            tokio::join!(receive, transmit);
        });
        let (mut socket, _) = tokio_tungstenite::connect_async(request(&url, true))
            .await
            .unwrap();
        socket
            .send(Message::Text(
                json!({"host":"localhost","port":port}).to_string().into(),
            ))
            .await
            .unwrap();
        assert_eq!(
            socket.next().await.unwrap().unwrap().into_text().unwrap(),
            r#"{"ok":true}"#
        );
        socket
            .send(Message::Ping(vec![7, 8, 9].into()))
            .await
            .unwrap();
        let mut early_download = Vec::new();
        loop {
            match socket.next().await.unwrap().unwrap() {
                Message::Pong(bytes) => {
                    assert_eq!(bytes.as_ref(), &[7u8, 8, 9]);
                    break;
                }
                Message::Binary(bytes) => early_download.extend_from_slice(&bytes),
                other => panic!("unexpected frame before pong: {other:?}"),
            }
        }
        let (mut sink, mut stream) = socket.split();
        let send = async {
            for chunk in upload.chunks(32768) {
                sink.send(Message::Binary(chunk.to_vec().into()))
                    .await
                    .unwrap();
            }
            sink.send(Message::Text(r#"{"eof":true}"#.into()))
                .await
                .unwrap();
        };
        let receive = async {
            let mut received = early_download;
            while let Some(frame) = stream.next().await {
                match frame.unwrap() {
                    Message::Binary(bytes) => received.extend_from_slice(&bytes),
                    Message::Pong(_) => {}
                    Message::Close(_) => break,
                    other => panic!("unexpected frame {other:?}"),
                }
            }
            assert_eq!(received, expected_download);
        };
        tokio::time::timeout(Duration::from_secs(15), async {
            tokio::join!(send, receive);
            peer.await.unwrap();
        })
        .await
        .unwrap();
        server.abort();
    }

    #[tokio::test]
    async fn revocation_read_only_and_service_stop_close_existing_tcp() {
        for change in ["revoke", "readonly", "stop"] {
            let (_directory, controller, url, server) = fixture().await;
            let target = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let port = target.local_addr().unwrap().port();
            let (mut socket, _) = tokio_tungstenite::connect_async(request(&url, true))
                .await
                .unwrap();
            socket
                .send(Message::Text(
                    json!({"host":"localhost","port":port}).to_string().into(),
                ))
                .await
                .unwrap();
            let (mut connection, _) = target.accept().await.unwrap();
            assert_eq!(
                socket.next().await.unwrap().unwrap().into_text().unwrap(),
                r#"{"ok":true}"#
            );
            match change {
                "revoke" => controller.auth.lock().await.devices.clear(),
                "readonly" => controller.publish_access_policy(&RemoteAccessSettings {
                    enabled: true,
                    read_only: true,
                    ..RemoteAccessSettings::default()
                }),
                _ => controller.running.store(false, Ordering::SeqCst),
            }
            controller.publish_event(json!({"method":"remote/authorizationChanged"}));
            let frame = tokio::time::timeout(Duration::from_secs(3), socket.next())
                .await
                .unwrap()
                .unwrap()
                .unwrap();
            let error: Value = serde_json::from_str(frame.to_text().unwrap()).unwrap();
            assert_eq!(error["ok"], false);
            let mut byte = [0u8; 1];
            assert_eq!(
                tokio::time::timeout(Duration::from_secs(3), connection.read(&mut byte))
                    .await
                    .unwrap()
                    .unwrap(),
                0
            );
            server.abort();
        }
    }
}
