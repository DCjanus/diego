use spin::Mutex;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Weak};
use std::task::{Poll, Waker};

pub struct Waiter(Arc<Mutex<State>>);

impl Waiter {
    pub fn construct(state: State) -> Self {
        Self(Arc::new(Mutex::new(state)))
    }

    pub fn weak(&self) -> Weak<Mutex<State>> {
        Arc::downgrade(&self.0)
    }
}

impl Future for Waiter {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> Poll<Self::Output> {
        let mut guard = self.0.lock();

        match &*guard {
            State::Done => Poll::Ready(()),
            _ => {
                *guard = State::Work(cx.waker().clone());
                Poll::Pending
            }
        }
    }
}

pub enum State {
    Init,
    Work(Waker),
    Done,
}

impl State {
    pub fn done(&mut self) {
        let value = std::mem::replace(self, Self::Done);
        if let Self::Work(waker) = value {
            waker.wake()
        }
    }
}
