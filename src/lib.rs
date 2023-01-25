#![no_std]
#![feature(waker_getters, const_waker)]
use core::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};
use core::{future::Future, ptr::null};
use core::{hint, pin::pin};

pub fn block_on<F: Future>(f: F) -> F::Output {
    static WAKER: Waker = {
        const RAW_WAKER: RawWaker = RawWaker::new(
            null(),
            &RawWakerVTable::new(|_| RAW_WAKER, |_| (), |_| (), |_| ()),
        );
        unsafe { Waker::from_raw(RAW_WAKER) }
    };

    let mut f = pin!(f);
    loop {
        match f.as_mut().poll(&mut Context::from_waker(&WAKER)) {
            Poll::Ready(r) => break r,
            Poll::Pending => hint::spin_loop(),
        }
    }
}

