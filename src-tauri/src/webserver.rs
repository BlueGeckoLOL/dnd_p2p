use std::{
    fs::File,
    io::{BufWriter, Write},
    net::SocketAddr,
    sync::Mutex,
};

use axum::{
    body::Bytes,
    extract::{DefaultBodyLimit, Multipart},
    routing::post,
    Router,
};
use axum_client_ip::{InsecureClientIp, SecureClientIp, SecureClientIpSource};
use log::info;
use once_cell::sync::OnceCell;
use tokio::{
    net::TcpListener,
    sync::broadcast::{Receiver, Sender},
};
use tower_http::trace::TraceLayer;

static BACK_TO_FRONT_CHANNEL: OnceCell<Mutex<Channel>> = OnceCell::new();
static FRONT_TO_BACK_CHANNEL: OnceCell<Mutex<Channel>> = OnceCell::new();

#[derive(Debug)]
pub struct Channel {
    pub tx: Sender<String>,
    pub rx: Receiver<String>,
}

pub async fn webserver(ip: String, b_channel: Channel, f_channel: Channel) {
    BACK_TO_FRONT_CHANNEL.set(Mutex::new(b_channel)).unwrap();
    FRONT_TO_BACK_CHANNEL.set(Mutex::new(f_channel)).unwrap();

    let app = Router::new()
        .route("/", post(accept_form))
        .layer(DefaultBodyLimit::disable())
        .layer(TraceLayer::new_for_http())
        .layer(SecureClientIpSource::ConnectInfo.into_extension());

    let listener = TcpListener::bind(ip.clone()).await.unwrap();

    info!("Web server started at {}", ip);
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await
    .unwrap();
}

async fn accept_form(_: InsecureClientIp, secure_ip: SecureClientIp, mut multipart: Multipart) {
    let b_channel = BACK_TO_FRONT_CHANNEL.get().unwrap();
    let b_tx = b_channel.lock().unwrap().tx.clone();
    let f_channel = FRONT_TO_BACK_CHANNEL.get().unwrap();
    let mut f_rx = f_channel.lock().unwrap().tx.subscribe();

    b_tx.send(secure_ip.0.to_string()).unwrap();
    loop {
        let recv = f_rx.try_recv();
        if let Ok(_) = recv {
            break;
        }
    }

    while let Some(field) = multipart.next_field().await.unwrap() {
        //let name = field.name().unwrap().to_string();
        let file_name = field.file_name().unwrap().to_string();
        let content_type = field.content_type().unwrap().to_string();
        let data = field.bytes().await.unwrap();

        info!(
            "`{file_name}`: Content-Type: `{content_type}`, {} bytes",
            data.len()
        );

        write_to_file(data, file_name).await;
    }
}

async fn write_to_file(content: Bytes, file_name: String) {
    let home_dir = home::home_dir().unwrap();
    let path = home_dir.join("Downloads").join(file_name);

    let f = File::create(&path).unwrap();
    let mut writer = BufWriter::new(f);
    writer.write_all(&content.to_vec()).unwrap();
    writer.flush().unwrap();

    info!("Saved at `{}`", path.to_str().unwrap());
}
