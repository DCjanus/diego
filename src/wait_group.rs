use std::sync::{Arc, Weak};

use spin::Mutex;

use crate::waiter::{State, Waiter};
use std::future::Future;

#[derive(Default, Clone)]
pub struct Group(Arc<Mutex<Inner>>);

impl Group {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn worker(&self) -> Worker {
        Worker::incr(&self.0)
    }

    pub fn wait(&self) -> impl Future<Output = ()> {
        let mut inner = self.0.lock();
        if inner.count == 0 {
            return Waiter::construct(State::Done);
        }

        let waiter = Waiter::construct(State::Init);
        inner.waiters.retain(|x| x.strong_count() > 0);
        inner.waiters.push(waiter.weak());
        waiter
    }
}

pub struct Worker(Arc<Mutex<Inner>>);

impl Worker {
    fn incr(inner: &Arc<Mutex<Inner>>) -> Self {
        let mut guard = inner.lock();
        guard.count += 1;
        Self(inner.clone())
    }
}

impl Clone for Worker {
    fn clone(&self) -> Self {
        Self::incr(&self.0)
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        let mut inner = self.0.lock();

        debug_assert!(inner.count > 0, "worker counter is too small");
        inner.count -= 1;

        if inner.count == 0 {
            inner
                .waiters
                .drain(..) // waiter has been done, remove it
                .filter_map(|x| x.upgrade())
                .for_each(|x| x.lock().done());
        }
    }
}

#[derive(Default)]
struct Inner {
    count: usize,
    waiters: Vec<Weak<Mutex<State>>>,
}
