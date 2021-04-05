use std::ops::Range;
use std::pin::Pin;
use std::time::{Duration, Instant};

use futures::FutureExt;
use tokio::time::sleep;

use diego::CancelTree;
use std::future::Future;

const BASE: Duration = Duration::from_millis(500);
const MISS: Duration = Duration::from_millis(50);
const STAT: Duration = Duration::from_millis(0);

fn time_base(
    range: Range<Duration>,
    stuff: impl Future<Output = ()> + Send + 'static,
) -> Pin<Box<dyn Future<Output = ()>>> {
    async move {
        let begin = Instant::now();
        let result = stuff.await;
        let cost = begin.elapsed();
        assert!(
            range.contains(&cost),
            "expect: {:?}, actual: {:?}",
            range,
            cost
        );
        result
    }
    .boxed()
}

async fn multi_waiter() {
    let root = CancelTree::new();
    let child = root.new_child();

    tokio::spawn(async move {
        sleep(BASE).await;
        root.cancel()
    });

    let tasks = (1..=5).into_iter().map(|_| child.wait());
    futures::future::join_all(tasks).await;
}

async fn multi_layer() {
    let root = CancelTree::new();
    let child = root.new_child();
    let grandchild = child.new_child();
    tokio::spawn(async move {
        sleep(BASE).await;
        root.cancel();
    });
    grandchild.wait().await;
}

async fn wait_after_done() {
    let root = CancelTree::new();
    root.cancel();
    root.wait().await;
}

async fn child_after_done() {
    let root = CancelTree::new();
    root.cancel();
    let child = root.new_child();
    child.wait().await;
}

async fn drop_canceled() {
    let root1 = CancelTree::new();
    let root2 = root1.clone();
    let child = root1.new_child();
    tokio::spawn(async move {
        sleep(BASE).await;
        drop(root1);
    });

    tokio::spawn(async move {
        sleep(BASE * 2).await;
        drop(root2);
    });

    child.wait().await;
}

#[tokio::test]
async fn test_main() {
    futures::future::join_all(vec![
        time_base(BASE..BASE + MISS, multi_waiter()),
        time_base(BASE..BASE + MISS, multi_layer()),
        time_base(STAT..MISS, wait_after_done()),
        time_base(STAT..MISS, child_after_done()),
        time_base(BASE * 2..BASE * 2 + MISS, drop_canceled()),
    ])
    .await;
}
