use tokio::sync::Mutex;
use std::sync::Arc;

#[tokio::main]
async fn main() {
    // kreiramo in ovijemo Mutex v Arc
    let data1 = Arc::new(Mutex::new(0));

    // kloniramo Arc
    let data2 = Arc::clone(&data1);

    let handle = tokio::spawn(async move {
        // pridobimo zaklep
        let mut guard = data2.lock().await;
        *guard += 1;
    });

    handle.await.unwrap();

    println!("Result: {}", *data1.lock().await);
}