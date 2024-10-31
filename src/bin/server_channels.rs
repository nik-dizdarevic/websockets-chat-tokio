use std::collections::HashMap;
use std::error::Error;
use std::future::Future;
use std::io::Cursor;
use bytes::{Buf, BytesMut};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::net::tcp::OwnedWriteHalf;
use tokio::select;
use tokio::sync::{mpsc};
use tokio::task::JoinHandle;
use uuid::Uuid;
use websockets::{FragmentedMessage, Frame, Request, VecExt, StatusCode};

type Result<T> = std::result::Result<T, Box<dyn Error + Send + Sync>>;
type ResponseFrame = Vec<u8>;
type Receiver<T> = mpsc::UnboundedReceiver<T>;
type Sender<T> = mpsc::UnboundedSender<T>;
type Users = HashMap<Uuid, Sender<ResponseFrame>>;

#[derive(Debug)]
enum Event {
    NewUser(Uuid, OwnedWriteHalf),
    Message(ResponseFrame, Recipient),
}

#[derive(Debug)]
enum Recipient {
    All,
    User(Uuid),
}

#[tokio::main]
async fn main() -> Result<()> {
    let listener = TcpListener::bind("127.0.0.1:7878").await?;
    println!("Listening on 127.0.0.1:7878");

    let (broker_tx, broker_rx) = mpsc::unbounded_channel();
    tokio::spawn(async move {
        broker_loop(broker_rx).await.expect("Broker failure");
    });

    loop {
        let (stream, _) = listener.accept().await.unwrap();
        spawn_and_log_error(handle_connection(stream, broker_tx.clone()));
    }
}

fn spawn_and_log_error<F>(future: F) -> JoinHandle<()>
where
    F: Future<Output = Result<()>> + Send + 'static
{
    tokio::spawn(async move {
        if let Err(e) = future.await {
            eprintln!("{}", e)
        }
    })
}

async fn handle_connection(mut stream: TcpStream, broker_tx: Sender<Event>) -> Result<()> {
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
        let result = read_loop(user, stream, buffer, &broker_tx).await;
        if result.is_err() {
            let close = Frame::Close(StatusCode::ProtocolError);
            broker_tx.send(Event::Message(close.response().unwrap(), Recipient::User(user))).unwrap();
        }
        result
    } else {
        Err("Not a valid websocket request".into())
    }
}

async fn read_loop(
    user: Uuid,
    stream: TcpStream,
    mut buffer: BytesMut,
    broker_tx: &Sender<Event>
) -> Result<()> {
    let (mut rd, wr) = stream.into_split();
    broker_tx.send(Event::NewUser(user, wr)).unwrap();

    let mut fragmented_message = FragmentedMessage::Text(vec![]);
    loop {
        let mut buff = Cursor::new(&buffer[..]);
        match Frame::parse(&mut buff, &mut fragmented_message) {
            Ok(frame) => {
                if let Some(response) = frame.response() {
                    if frame.is_text() || frame.is_binary() || frame.is_continuation() {
                        broker_tx.send(Event::Message(response, Recipient::All)).unwrap();
                        fragmented_message = FragmentedMessage::Text(vec![]);
                    } else {
                        broker_tx.send(Event::Message(response, Recipient::User(user))).unwrap();
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

async fn writer_loop(mut user_rx: Receiver<ResponseFrame>, mut wr: OwnedWriteHalf) -> Result<()> {
    while let Some(message) = user_rx.recv().await {
        wr.write_all(&message).await?;
        wr.flush().await?;
        if message.is_close() {
            break;
        }
    }
    Ok(())
}

async fn broker_loop(mut broker_rx: Receiver<Event>) -> Result<()> {
    let (disconnect_tx, mut disconnect_rx) = mpsc::unbounded_channel();
    let mut users= Users::new();

    loop {
        let event = select! {
            event = broker_rx.recv() => match event {
                None => break,
                Some(event) => event,
            },
            user = disconnect_rx.recv() => match user {
                None => break,
                Some(user) => {
                      // println!("Goodbye user: {:?}", user);
                    users.remove(&user);
                    continue;
                }
            }
        };
        match event {
            Event::NewUser(user, wr) => {
                let (user_tx, user_rx) = mpsc::unbounded_channel();
                users.insert(user, user_tx);
                let disconnect_tx = disconnect_tx.clone();
                spawn_and_log_error(async move {
                    let result = writer_loop(user_rx, wr).await;
                    disconnect_tx.send(user).unwrap();
                    result
                });
            }
            Event::Message(message, recipient) => match recipient {
                Recipient::All => {
                    for user_tx in users.values() {
                        if let Err(e) = user_tx.send(message.clone()) {
                            eprintln!("Failed sending to other users: {}", e);
                        }
                    }
                }
                Recipient::User(user) => {
                    if let Some(user_tx) = users.get(&user) {
                        user_tx.send(message).unwrap();
                    }
                }
            }
        }
    }

    Ok(())
}