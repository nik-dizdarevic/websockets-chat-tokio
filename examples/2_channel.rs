use tokio::sync::mpsc;

#[tokio::main]
async fn main() {
    // kreiramo kanal
    let (tx, mut rx) = mpsc::unbounded_channel();

    // kloniramo oddajnik
    let tx2 = tx.clone();

    tokio::spawn(async move {
        // pošljemo vrednost
        tx.send("sending from first handle").unwrap();
    });

    tokio::spawn(async move {
        // pošljemo vrednost
        tx2.send("sending from second handle").unwrap();
    });

    // sprejmemo vrednost
    while let Some(message) = rx.recv().await {
        println!("Got = {}", message);
    }
}