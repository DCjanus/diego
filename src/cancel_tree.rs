use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Weak};
use std::task::{Poll, Waker};

use spin::{Mutex, MutexGuard};

#[derive(Clone)]
pub struct CancelTree(Arc<Mutex<Inner>>);

impl CancelTree {
    pub fn root() -> Self {
        Self::construct(Inner {
            waiters: vec![],
            children: vec![],
            canceled: false,
        })
    }

    pub fn new_child(&self) -> Self {
        let mut inner = self.inner();

        if inner.canceled {
            return Self::construct(Inner {
                waiters: vec![],
                children: vec![],
                canceled: true,
            });
        }

        let child = Self::root();
        inner.children.push(child.weak());
        child
    }

    pub fn cancel(&self) {
        self.inner().cancel()
    }

    pub fn canceled(&self) -> bool {
        self.inner().canceled
    }

    pub fn wait(&self) -> impl Future<Output = ()> {
        let mut inner = self.inner();
        if inner.canceled {
            return Waiter::construct(State::Canceled);
        }

        let waiter = Waiter::construct(State::Init);
        inner.waiters.push(waiter.weak());
        waiter
    }

    fn construct(inner: Inner) -> Self {
        Self(Arc::new(Mutex::new(inner)))
    }

    fn inner(&self) -> MutexGuard<Inner> {
        self.0.lock()
    }

    fn weak(&self) -> Weak<Mutex<Inner>> {
        Arc::downgrade(&self.0)
    }
}

struct Inner {
    waiters: Vec<Weak<Mutex<State>>>,
    children: Vec<Weak<Mutex<Inner>>>,
    canceled: bool,
}

impl Inner {
    fn cancel(&mut self) {
        if self.canceled {
            return;
        }
        self.canceled = true;

        std::mem::take(&mut self.children)
            .into_iter()
            .filter_map(|x| x.upgrade())
            .for_each(|x| x.lock().cancel());

        std::mem::take(&mut self.waiters)
            .into_iter()
            .filter_map(|x| x.upgrade())
            .for_each(|x| x.lock().done());
    }
}

impl Drop for Inner {
    fn drop(&mut self) {
        self.cancel()
    }
}

struct Waiter(Arc<Mutex<State>>);

impl Waiter {
    fn construct(state: State) -> Self {
        Self(Arc::new(Mutex::new(state)))
    }

    fn weak(&self) -> Weak<Mutex<State>> {
        Arc::downgrade(&self.0)
    }
}

impl Future for Waiter {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> Poll<Self::Output> {
        let mut guard = self.0.lock();

        match &*guard {
            State::Canceled => Poll::Ready(()),
            _ => {
                *guard = State::Running(cx.waker().clone());
                Poll::Pending
            }
        }
    }
}

enum State {
    Init,
    Running(Waker),
    Canceled,
}

impl State {
    fn done(&mut self) {
        let value = std::mem::replace(self, Self::Canceled);
        if let Self::Running(waker) = value {
            waker.wake()
        }
    }
}

#[cfg(test)]
mod tests {
    use std::ops::Range;
    use std::time::{Duration, Instant};

    use futures::FutureExt;
    use tokio::time::sleep;

    use super::*;

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
        let root = CancelTree::root();
        let child = root.new_child();

        tokio::spawn(async move {
            sleep(BASE).await;
            root.cancel()
        });

        let tasks = (1..=5).into_iter().map(|_| child.wait());
        futures::future::join_all(tasks).await;
    }

    async fn multi_layer() {
        let root = CancelTree::root();
        let child = root.new_child();
        let grandchild = child.new_child();
        tokio::spawn(async move {
            sleep(BASE).await;
            root.cancel();
        });
        grandchild.wait().await;
    }

    async fn wait_after_done() {
        let root = CancelTree::root();
        root.cancel();
        root.wait().await;
    }

    async fn child_after_done() {
        let root = CancelTree::root();
        root.cancel();
        let child = root.new_child();
        child.wait().await;
    }

    async fn drop_canceled() {
        let root1 = CancelTree::root();
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
}
