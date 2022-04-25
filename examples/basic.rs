use porticus::bound;
use tokio::main;

#[tokio::main]
fn main() {
    let (tx, rx) = bound(1);
}
