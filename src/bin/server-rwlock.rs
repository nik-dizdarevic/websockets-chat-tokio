use std::collections::HashMap;
use std::error::Error;
use std::sync::Arc;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{RwLock, mpsc};
use uuid::Uuid;
use std::io::Cursor;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use bytes::{BytesMut, Buf};
use tokio::net::tcp::OwnedReadHalf;
use websockets::{Frame, FragmentedMessage, Request, VecExt};

type Result<T> = std::result::Result<T, Box<dyn Error + Send + Sync>>;
type ResponseFrame = Vec<u8>;
type Sender<T> = mpsc::UnboundedSender<T>;
type Users = Arc<RwLock<HashMap<Uuid, Sender<ResponseFrame>>>>;

#[tokio::main]
async fn main() -> Result<()> {
    let listener = TcpListener::bind("127.0.0.1:7878").await?;
    println!("Listening on 127.0.0.1:7878");

    let users = Users::default();
    loop {
        let (stream, _) = listener.accept().await.unwrap();
        let users = users.clone();
        tokio::spawn(async move {
            handle_connection(stream, users).await.expect("Failure when handling connection");
        });
    }
}

async fn handle_connection(mut stream: TcpStream, users: Users) -> Result<()> {
    let user = Uuid::new_v4();
    let mut buffer = BytesMut::with_capacity(4096);

    // println!("Welcome user {:?}", user);

    if 0 == stream.read_buf(&mut buffer).await? {
        return Err("Connection closed by remote.".into());
    }

    let request = Request::new(&buffer)?;
    if let Some(response) = request.response() {
        stream.write_all(response.as_bytes()).await?;
        stream.flush().await?;
        buffer.clear();
        handle_websocket_frames(users, user, stream, buffer).await
    } else {
        Err("Not a valid websocket request".into())
    }
}

async fn handle_websocket_frames(
    users: Users,
    user: Uuid,
    stream: TcpStream,
    buffer: BytesMut
) -> Result<()> {
    let (tx, mut rx) = mpsc::unbounded_channel();
    users.write().await.insert(user, tx);

    let (rd, mut wr) = stream.into_split();

    let users_rd = users.clone();
    tokio::spawn(async move {
        if let Err(e) = read_loop(buffer, rd, &users_rd, user).await {
            println!("Error: {:?}", e);
            disconnect(user, &users_rd).await;
        }
    });

    while let Some(response) = rx.recv().await {
        if let Err(e) = wr.write_all(&response).await {
            println!("Error: {:?}", e);
            disconnect(user, &users).await;
            break;
        }
        wr.flush().await.unwrap();
        if response.is_close() {
            disconnect(user, &users).await;
            break;
        }
    }

    Ok(())
}

async fn read_loop(
    mut buffer: BytesMut,
    mut rd: OwnedReadHalf,
    users: &Users,
    user: Uuid
) -> Result<()> {
    let mut fragmented_message = FragmentedMessage::Text(vec![]);
    loop {
        let mut buff = Cursor::new(&buffer[..]);
        match Frame::parse(&mut buff, &mut fragmented_message) {
            Ok(frame) => {
                if let Some(response) = frame.response() {
                    if frame.is_text() || frame.is_binary() || frame.is_continuation() {
                        for tx in users.read().await.values() {
                            tx.send(response.clone()).expect("Failed to send message");
                        }
                        fragmented_message = FragmentedMessage::Text(vec![]);
                    } else {
                        if let Some(tx) = users.read().await.get(&user) {
                            tx.send(response).expect("Failed to send message");
                        }
                        if frame.is_close() {
                            return Ok(());
                        }
                    }
                }
                buffer.advance(buff.position() as usize);
            }
            Err(_) => {
                if 0 == rd.read_buf(&mut buffer).await? {
                    return Err("Connection closed by remote.".into());
                }
            }
        }
    }
}

async fn disconnect(user: Uuid, users: &Users) {
    // println!("Goodbye user: {:?}", user);
    users.write().await.remove(&user);
}