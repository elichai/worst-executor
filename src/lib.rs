#![cfg_attr(not(test), no_std)]
#![feature(waker_getters, const_waker)]
#![cfg_attr(test, feature(future_join))]

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

#[cfg(test)]
mod tests {
    use crate::block_on;
    use core::{future, pin::pin, task::Poll};
    use futures_lite::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
    #[test]
    fn test_block_on_trivial() {
        assert_eq!(block_on(async { 42 }), 42);
    }

    #[test]
    fn test_block_on_n_tries() {
        for n in 0..420 {
            let mut index = 0;
            let future = future::poll_fn(|ctx| {
                if index < n {
                    index += 1;
                    ctx.waker().wake_by_ref();
                    Poll::Pending
                } else {
                    Poll::Ready(index)
                }
            });
            let ret = block_on(async {
                assert_eq!(future.await, n);
                n
            });
            assert_eq!(ret, n);
        }
    }

    async fn write_read_eq<W: AsyncWrite, R: AsyncRead>(writer: W, reader: R, data: &[u8]) {
        let (mut writer, mut reader) = (pin!(writer), pin!(reader));
        let write = async {
            writer.write_all(data).await.unwrap();
            writer.flush().await.unwrap();
        };
        let read = async {
            let mut buf = vec![0u8; data.len()];
            reader.read_exact(&mut buf).await.unwrap();
            buf
        };
        let (_, buf) = future::join!(write, read).await;
        assert_eq!(buf, data);
    }

    #[test]
    fn test_async_fs() {
        use async_fs::File;
        let data = b"Hello, world! from my async executor";
        let path = "./target/test_file";
        // Cleanup
        let _ = block_on(async_fs::remove_file(path));
        block_on(async{
            {
                let writer = File::create(path).await.unwrap();
                let reader = File::open(path).await.unwrap();
                write_read_eq(writer, reader, data).await;
            }
            // Cleanup
            async_fs::remove_file(path).await.unwrap();
        });
    }

        let read_data = block_on(async {
            let mut file = File::open(path).await.unwrap();
            let mut buf = Vec::new();
            file.read_to_end(&mut buf).await.unwrap();
            buf
        });
        assert_eq!(read_data, data);
        block_on(async_fs::remove_file(path)).unwrap()
    }

}
