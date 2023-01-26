# Worst Executor
[![Build status](https://github.com/elichai/worst-executor/actions/workflows/ci.yaml/badge.svg)](https://github.com/elichai/worst-executor/actions)
[![Latest version](https://img.shields.io/crates/v/worst-executor.svg)](https://crates.io/crates/worst-executor)
![License](https://img.shields.io/crates/l/worst-executor.svg)
[![dependency status](https://deps.rs/repo/github/elichai/worst-executor/status.svg)](https://deps.rs/repo/github/elichai/worst-executor)

The simplest async executor possible. <br>
This crate provides a single function, `block_on`, which takes a future and
blocks the current thread until the future is resolved. <br>
The way it works is by "spin-looping" over the `poll` method until it is ready.

The nice thing about this is that it optimizes very well,
 for example `worst_executor::block_on(async { 42 })` compiles to a single `mov` instruction. <br>

The bad thing about this is that it does not actually do any scheduling, meaning that if you
wait on a future that never resolves, your program will hang. which is why you should probably not use this.

Note that because of its simplicity, the library only uses `core` and does not require `std` or `alloc`
nor does it have any dependencies, and is literally 16 lines of code.

* [Documentation](https://docs.rs/worst-executor)

## Usage

Add this to your `Cargo.toml`:

```toml
[dependencies]
worst-executor = "0.1"
```
Then in your main.rs:

```rust

fn main() {
    worst_executor::block_on(async {
        // Your async code goes here.
    });
}
```

### Use cases
Say you're using a library which returns a future, but you know it does no I/O, 
and you want to use it in a synchronous context without bringing in a full async runtime like tokio/smol/async-std <br>
You can use `worst_executor::block_on` to block the current thread until the future is resolved.

Another scenario can be that you want to run an "event loop" in a single thread using `async` rust.
so you can use the `core::future::join` macro together with the `futures::select` macro
to handle the control flow of your program, while always running on a single thread and never yielding.

### Single threaded tcp server
```rust
use async_net::{TcpListener, TcpStream};
use futures::{stream::FuturesUnordered, AsyncReadExt, AsyncWriteExt, StreamExt};
use worst_executor::block_on;

block_on(async {
    let listener = TcpListener::bind("127.0.0.1:8080").await.unwrap();
    let mut connection_handlers = FuturesUnordered::new();
    // This stream is infinite so it's OK to call fuse.
    let mut listener = listener.incoming().fuse();
    loop {
        futures::select! {
            new_connection = listener.select_next_some() => connection_handlers.push(handle_connection(new_connection?)),
            socket = connection_handlers.select_next_some() =>
                if let Some(socket) = socket {
                    connection_handlers.push(handle_connection(socket));
                },
        }
    }
})

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
    stream.write_all(&buf[0..n]).await
            .map_err(|e| eprintln!("failed to write to the socket {e:?}"))
            .map(|()| stream)
            .ok()
}
```