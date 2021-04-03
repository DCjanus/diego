use std::future::Future;
use std::ops::DerefMut;
use std::pin::Pin;
use std::sync::{Arc, Weak};
use std::task::{Poll, Waker};

use spin::{Mutex, MutexGuard};

#[derive(Default, Clone)]
pub struct CancelTree(Arc<Mutex<Inner>>);

impl CancelTree {
    pub fn root() -> Self {
        Self::default()
    }

    pub fn child(&self) -> Self {
        let mut inner = self.inner();

        match &mut *inner {
            Inner::Init { children, .. } => {
                let child = Self::root();
                children.push(child.weak());
                child
            }
            Inner::Done => Self(Arc::new(Mutex::new(Inner::Done))),
        }
    }

    pub fn done(&self) {
        let mut inner = self.inner();
        if inner.is_done() {
            return;
        }

        let cancel_tree = std::mem::replace(inner.deref_mut(), Inner::Done);
        let (waiters, children) = match cancel_tree {
            Inner::Init { waiters, children } => (waiters, children),
            Inner::Done => {
                unreachable!()
            }
        };

        children
            .into_iter()
            .filter_map(|x| x.upgrade())
            .map(Self)
            .for_each(|x| x.done());

        waiters
            .into_iter()
            .filter_map(|x| x.upgrade())
            .for_each(|x| x.lock().done());
    }

    pub fn is_done(&self) -> bool {
        self.inner().is_done()
    }

    pub fn wait(&self) -> impl Future<Output = ()> {
        let mut inner = self.inner();

        let waiters = match inner.deref_mut() {
            Inner::Init { waiters, .. } => waiters,
            Inner::Done => return Waiter::done(),
        };
        let waiter = Waiter::init();
        waiters.push(waiter.weak());
        waiter
    }

    fn inner(&self) -> MutexGuard<Inner> {
        self.0.lock()
    }

    fn weak(&self) -> Weak<Mutex<Inner>> {
        Arc::downgrade(&self.0)
    }
}

enum Inner {
    Init {
        waiters: Vec<Weak<Mutex<WaiterImpl>>>,
        children: Vec<Weak<Mutex<Inner>>>,
    },
    Done,
}

impl Inner {
    fn is_done(&self) -> bool {
        matches!(self, Self::Done)
    }
}

impl Default for Inner {
    fn default() -> Self {
        Self::Init {
            waiters: vec![],
            children: vec![],
        }
    }
}

struct Waiter(Arc<Mutex<WaiterImpl>>);

impl Waiter {
    fn init() -> Self {
        Waiter(Arc::new(Mutex::new(WaiterImpl::Init)))
    }

    fn done() -> Self {
        Waiter(Arc::new(Mutex::new(WaiterImpl::Done)))
    }

    fn weak(&self) -> Weak<Mutex<WaiterImpl>> {
        Arc::downgrade(&self.0)
    }
}

impl Future for Waiter {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> Poll<Self::Output> {
        let mut guard = self.0.lock();

        match &*guard {
            WaiterImpl::Done => Poll::Ready(()),
            _ => {
                *guard = WaiterImpl::Wait(cx.waker().clone());
                Poll::Pending
            }
        }
    }
}

enum WaiterImpl {
    Init,
    Wait(Waker),
    Done,
}

impl WaiterImpl {
    fn done(&mut self) {
        let value = std::mem::replace(self, Self::Done);
        if let Self::Wait(waker) = value {
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
        let child = root.child();

        tokio::spawn(async move {
            sleep(BASE).await;
            root.done()
        });

        let tasks = (1..=5).into_iter().map(|_| child.wait());
        futures::future::join_all(tasks).await;
    }

    async fn multi_layer() {
        let root = CancelTree::root();
        let child = root.child();
        let grandchild = child.child();
        tokio::spawn(async move {
            sleep(BASE).await;
            root.done();
        });
        grandchild.wait().await;
    }

    async fn wait_after_done() {
        let root = CancelTree::root();
        root.done();
        root.wait().await;
    }

    async fn child_after_done() {
        let root = CancelTree::root();
        root.done();
        let child = root.child();
        child.wait().await;
    }

    #[tokio::test]
    async fn test_main() {
        futures::future::join_all(vec![
            time_base(BASE..BASE + MISS, multi_waiter()),
            time_base(BASE..BASE + MISS, multi_layer()),
            time_base(STAT..MISS, wait_after_done()),
            time_base(STAT..MISS, child_after_done()),
        ])
        .await;
    }
}
