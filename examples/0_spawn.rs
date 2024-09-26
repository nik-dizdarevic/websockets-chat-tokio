#[tokio::main]
async fn main() {
    let handle = tokio::spawn(async {
        // tukaj opravimo nekaj asinhronega dela
        "return value"
    });

    // tukaj opravimo nekaj drugega dela

    let out = handle.await.unwrap();
    println!("Got {}", out);
}