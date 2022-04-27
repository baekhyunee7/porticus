use porticus::bound;
use tokio::main;

#[tokio::main]
async fn main() {
    let (tx, rx) = bound(100);
    for i in 0..100 {
        let tx = tx.clone();
        tokio::spawn(async move {
            tx.send_future(i).await.unwrap();
        });
    }
    let mut sum = 0;
    for _ in 0..100 {
        let item = rx.receive_future().await.unwrap();
        println!("{}: {}", sum, item);
        sum += 1;
    }
}
