use std::future::Future;
use std::sync::{Arc, Weak};

use spin::{Mutex, MutexGuard};

use crate::waiter::{State, Waiter};

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
            return Waiter::construct(State::Done);
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
