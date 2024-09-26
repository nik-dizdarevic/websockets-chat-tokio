use tokio::sync::oneshot;

#[tokio::main]
async fn main() {
    // kreiramo dva kanala
    let (tx1, rx1) = oneshot::channel();
    let (tx2, rx2) = oneshot::channel();

    tokio::spawn(async {
        let _ = tx1.send("one");
    });

    tokio::spawn(async {
        let _ = tx2.send("two");
    });

    // hkrati čakamo na oba kanala
    let out = tokio::select! {
        val = rx1 => {
            format!("rx1 completed first with {:?}", val.unwrap())
        },
        val = rx2 => {
            format!("rx2 completed first with {:?}", val.unwrap())
        }
    };

    println!("Got = {}", out);
}