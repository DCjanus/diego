use std::future::Future;
use std::ops::Range;
use std::pin::Pin;
use std::time::{Duration, Instant};

use futures::FutureExt;
use tokio::time::sleep;

use diego::Group;

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

async fn basic_test() {
    let group = Group::new();
    let worker1 = group.worker();
    let worker2 = group.worker();
    tokio::spawn(async move {
        sleep(BASE).await;
        drop(worker1)
    });
    tokio::spawn(async move {
        sleep(BASE).await;
        drop(worker2)
    });
    group.wait().await;
}

async fn wait_nothing() {
    let group = Group::new();
    group.wait().await;
}

async fn wait_after_done() {
    let group = Group::new();
    group.wait().await;
    let worker = group.worker();
    tokio::spawn(async move {
        sleep(BASE).await;
        drop(worker)
    });
    group.wait().await;
}

async fn wait_and_wait() {
    let group = Group::new();

    let worker = group.worker();
    tokio::spawn(async move {
        sleep(BASE).await;
        drop(worker)
    });
    group.wait().await;

    let worker = group.worker();
    tokio::spawn(async move {
        sleep(BASE).await;
        drop(worker)
    });
    group.wait().await;
}

async fn concurrency_wait() {
    let group1 = Group::new();
    let worker = group1.worker();
    let group2 = group1.clone();

    tokio::spawn(async move {
        sleep(BASE).await;
        drop(worker)
    });

    futures::future::join_all(vec![group1.wait(), group2.wait()]).await;
}

#[tokio::test]
async fn test_main() {
    futures::future::join_all(vec![
        time_base(BASE..BASE + MISS, basic_test()),
        time_base(STAT..STAT + MISS, wait_nothing()),
        time_base(BASE..BASE + MISS, wait_after_done()),
        time_base(BASE * 2..BASE * 2 + MISS, wait_and_wait()),
        time_base(BASE..BASE + MISS, concurrency_wait()),
    ])
    .await;
}
