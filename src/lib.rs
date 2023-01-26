//! The worst async executor you could think of.
//!
//! This crate provides a single function, `block_on`, which takes a future and
//! blocks the current thread until the future is resolved.
//! The way it works is by "spin-looping" over the `poll` method until it is ready.
//!
//! The nice thing about this is that it optimizes very well,
//! for example `worst_executor::block_on(async { 42 })` compiles to a single `mov` instruction.
//!
//! The bad thing about this is that it does not actually do any scheduling, meaning that if you
//! wait on a future that never resolves, your program will hang. which is why you should probably not use this.
//!
//! Note that because of its simplicity, the library only uses `core` and does not require `std` or `alloc`
//! and is literally 16 lines of code.
//!
//! This can become more useful using the `core::future::join` macro
//! which allows you to wait on multiple futures at once (each time polling a different future).
//!
//! Currently this library requires rust *nightly* for the `pin` macro, the `join` macro(used in tests) and the `const_waker` feature (required for complete optimization of `block_on`)
//!
//! # Examples
//! ```rust
//! use worst_executor::block_on;
//!
//! let val = block_on(async { 42 });
//! assert_eq!(val, 42);
//! ```
//!
//! ```rust
//! #![feature(future_join)]
//! use worst_executor::block_on;
//! use core::future::join;
//!
//!  let (sender, receiver) = async_channel::bounded(1);
//!  block_on(async {
//!     let send_20_fut = async {
//!         for i in 0..20 {
//!             sender.send(i).await.unwrap();
//!         }
//!     };
//!     let recv_20_fut = async {
//!         for i in 0..20 {
//!             assert_eq!(receiver.recv().await.unwrap(), i);
//!         }
//!     };
//!     join!(send_20_fut, recv_20_fut).await;
//!  });
//! ```

#![cfg_attr(not(test), no_std)]
#![feature(const_waker)]
#![cfg_attr(test, feature(future_join))]

use core::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};
use core::{future::Future, ptr::null};
use core::{hint, pin::pin};

/// Runs the given future to completion on the current thread.
/// This function will block the current thread until the future is resolved.
/// The way it works is by "spin-looping" over the `poll` method until it is ready.
///
/// It will not do any scheduling, nor will it launch any threads.
///
/// # Examples
/// ```rust
/// use worst_executor::block_on;
/// block_on(async {
///    println!("Hello, world!");
/// })
/// ```
///
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
    use core::future::join;
    use core::{future, pin::pin, task::Poll};
    use futures::{
        stream::FuturesUnordered, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, FutureExt,
        StreamExt,
    };

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
        block_on(async {
            {
                let writer = File::create(path).await.unwrap();
                let reader = File::open(path).await.unwrap();
                write_read_eq(writer, reader, data).await;
            }
            // Cleanup
            async_fs::remove_file(path).await.unwrap();
        });
    }

    #[cfg(unix)]
    #[test]
    fn test_async_unix_socket() {
        use async_net::unix::{UnixListener, UnixStream};
        let path = "./target/test_socket.sock";
        let data = b"Hello, world! from my async executor in UNIX stream";
        // Cleanup
        let _ = block_on(async_fs::remove_file(path));
        let listener = UnixListener::bind(path).unwrap();

        block_on(async {
            let (sender, receiver) =
                future::join!(listener.accept(), UnixStream::connect(path)).await;
            write_read_eq(sender.unwrap().0, receiver.unwrap(), data).await;
            // Cleanup
            async_fs::remove_file(path).await.unwrap();
        });
    }

    #[test]
    fn test_async_tcp_socket() {
        use async_net::{TcpListener, TcpStream};
        let localhost = "127.0.0.1";
        let data = b"Hello, world! from my async executor in TCP stream";

        block_on(async {
            let listener = TcpListener::bind((localhost, 0)).await.unwrap();
            let port = listener.local_addr().unwrap().port();

            let (sender, receiver) =
                future::join!(listener.accept(), TcpStream::connect((localhost, port))).await;
            write_read_eq(sender.unwrap().0, receiver.unwrap(), data).await;
        });
    }

    #[test]
    fn test_async_udp_socket() {
        use async_net::UdpSocket;
        let localhost = "127.0.0.1";
        let data = b"Hello, world! from my async executor in UDP stream";

        block_on(async {
            let sender = UdpSocket::bind((localhost, 0)).await.unwrap();
            let receiver = UdpSocket::bind((localhost, 0)).await.unwrap();
            let sender_port = sender.local_addr().unwrap().port();
            let receiver_port = receiver.local_addr().unwrap().port();
            receiver.connect((localhost, sender_port)).await.unwrap();
            sender.connect((localhost, receiver_port)).await.unwrap();
            let mut buf = vec![0u8; data.len()];
            let (sender_res, receiver_res) =
                join!(sender.send(data), receiver.recv(&mut buf)).await;
            sender_res.unwrap();
            receiver_res.unwrap();
            assert_eq!(buf, data);
        });
    }

    #[test]
    fn test_async_unbounded_channel() {
        let data = b"Hello, world! from my async executor in a channel";
        let (sender, receiver) = async_channel::unbounded();
        block_on(async {
            let (sender_res, ret) = join!(sender.send(data), receiver.recv()).await;
            sender_res.unwrap();
            assert_eq!(ret.unwrap(), data);
        });
    }

    #[test]
    fn test_async_bounded_channel() {
        let (sender, receiver) = async_channel::bounded(1);
        block_on(async {
            let send_20_fut = async {
                for i in 0..20 {
                    sender.send(i).await.unwrap();
                }
            };
            let recv_20_fut = async {
                for i in 0..20 {
                    assert_eq!(receiver.recv().await.unwrap(), i);
                }
            };
            join!(send_20_fut, recv_20_fut).await;
        });
    }

    #[test]
    fn test_tcp_server() {
        use async_net::{TcpListener, TcpStream};

        async fn handle_connection(mut stream: TcpStream) -> Option<TcpStream> {
            let mut buf = [0u8; 1024];
            let n = match stream.read(&mut buf).await {
                Ok(n) if n == 0 => return Some(stream),
                Ok(n) => n,
                Err(e) => {
                    eprintln!("failed to read from the socket {e:?}");
                    return None;
                }
            };
            // Write the data back
            stream.write_all(&buf[0..n]).await.map_err(|e| eprintln!("failed to write to the socket {e:?}")).map(|()| stream).ok()
        }
        let localhost = "127.0.0.1";
        let listener = crate::block_on(TcpListener::bind((localhost, 0))).unwrap();
        let port = listener.local_addr().unwrap().port();

        let res = block_on(async move {
            let mut readers = futures::future::join_all((0..10).map(|i| async move {
                let mut stream = TcpStream::connect((localhost, port)).await.unwrap();
                let data =
                    format!("Hello, world! from my async executor in TCP stream number: {i}");
                stream.write_all(data.as_bytes()).await.unwrap();
                let mut buf = vec![0u8; data.len()];
                stream.read_exact(&mut buf).await.unwrap();
                assert_eq!(buf, data.as_bytes());
                i
            }))
            .fuse();
            // This stream is inifinite so same to call fuse.
            let mut connection_handlers = FuturesUnordered::new();
            let mut listener = listener.incoming().fuse();
            loop {
                futures::select! {
                    new_connection = listener.select_next_some() => {
                        connection_handlers.push(handle_connection(new_connection.unwrap()));
                    },
                    socket = connection_handlers.select_next_some() => {
                        if let Some(socket) = socket {
                            connection_handlers.push(handle_connection(socket));
                        }
                    },
                    result = readers => {
                        break result
                    }
                }
            }
        });

        assert_eq!(res, [0, 1, 2, 3, 4, 5, 6, 7, 8, 9]);
    }
}
