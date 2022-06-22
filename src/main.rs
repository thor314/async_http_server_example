// need to change our dependencies up a bit to use async stuff, either from std or tokio. We'll use std.
// Also, I'd prefer to take advantage of ergonomics with ?, taking some liberties.

// #[macro_use]
// use async_std;


use async_std::{
    io::{ReadExt, WriteExt},
    net::{TcpListener},
};
use futures::StreamExt;

// We're going to use the async main runtime, similar to Tokio I think. see Cargo toml, added:
// [dependencies.async-std]
// version = "1.6"
// features = ["attributes"]

// pretty good main, without handling connections on indep threads:

// hmm, rust-ana thinks this sucks, but it build, what a loser
// #[async_std::main]
// async fn main() -> Result<(), Box<dyn Error>> {
//     let listener = TcpListener::bind("127.0.0.1:7878").await?;

//     // From the not async:
//     // for stream in listener.incoming() {
//     //     let stream = stream.unwrap();
//     //     handle_connection(stream);
//     // }
//     // take advantage of the utilities in the futures crate to make this async.
//     // `for_each_concurrent` is an async utility to handle streams in parallel.
//     // Diff from par_iter? Idk. Anyways, we want something like this:
//     // listener.incoming().for_each_concurrent(None, |tcpstream| async move{ // handle })
//     // apparently, for_each_concurrent gets mad if I progagate results from `handle_connection`, so don't do that I guess
//     StreamExt::for_each_concurrent(listener.incoming(), None, |tcpstream| async move {
//         // println!("tcpstream conn: {tcpstream:?}");
//         handle_connection(&tcpstream.unwrap()).await; // let's try passing this by reference
//     }).await;

//     Ok(())
// }

// swap this out to something generic, so we can write mocktest below:
// async fn handle_connection(mut stream: TcpStream) {
async fn handle_connection<T: async_std::io::Read + async_std::io::Write + Unpin>(mut stream: T) {
    let mut buffer = [0; 1024];
    // 1. async read to buffer. From:
    // stream.read(&mut buffer).unwrap();
    // To: (note the change of imports)
    stream.read(&mut buffer).await.unwrap();

    let get = b"GET / HTTP/1.1\r\n";

    // 2. handle asyncronously
    let (status_line, filename) = if buffer.starts_with(get) {
        ("HTTP/1.1 200 OK\r\n\r\n", "hello.html")
    } else {
        ("HTTP/1.1 404 NOT FOUND\r\n\r\n", "404.html")
    };
    // 3. async read to string
    let contents = std::fs::read_to_string(filename).unwrap();

    // Write response back to the stream,
    // and flush the stream to ensure the response is sent back to the client
    let response = format!("{status_line}{contents}");
    // 4. async write back response contents?
    stream.write_all(response.as_bytes()).await.unwrap();
    stream.flush().await.unwrap();
}

// And that's pretty much all there is to it. Swap out a runtime and change some traits, and we're good. Wow.
// Okay, there's at least one more area improve: serving in parallel. We should be handling by spawning threads. for_each_concurrent does not do that for us.

// use async_std::task::spawn;

#[async_std::main]
async fn main() {
    let listener = TcpListener::bind("127.0.0.1:7878").await.unwrap();
    listener
        .incoming()
        .for_each_concurrent(/* limit */ None, |stream| async move {
            let stream = stream.unwrap();
            // But std::spawn or async_std spawn? async_std spawn. std spawn doesn't expect an async handle.
            // We also end up having to take ownership now, we can't pass by reference if we're moving it into a thread.
            async_std::task::spawn(handle_connection(stream));
            // std::thread::spawn(handle_connection(stream)); // nope.
        })
        .await;
}

// a handle_conn that returns a Result; this made for_each_concurrent mad.

// async fn handle_connection(mut stream: TcpStream) -> Result<(), Box<dyn Error>> {
//     let mut buffer = [0; 1024];
//     // 1. async read to buffer. From:
//     // stream.read(&mut buffer).unwrap();
//     // To: (note the change of imports)
//     stream.read(&mut buffer).await.unwrap();

//     let get = b"GET / HTTP/1.1\r\n";

//     // 2. handle asyncronously
//     let (status_line, filename) = if buffer.starts_with(get) {
//         ("HTTP/1.1 200 OK\r\n\r\n", "hello.html")
//     } else {
//         ("HTTP/1.1 404 NOT FOUND\r\n\r\n", "404.html")
//     };
//     // 3. async read to string
//     let contents = fs::read_to_string(filename).unwrap();

//     // Write response back to the stream,
//     // and flush the stream to ensure the response is sent back to the client
//     let response = format!("{status_line}{contents}");
//     // 4. async write back response contents?
//     stream.write_all(response.as_bytes()).await?;
//     stream.flush().await?;
//     Ok(())
// }

// Finally, we should test our thing.
#[cfg(test)]
mod test {
    use super::*;
    use futures::task::{Context, Poll};
    use futures::{io::Error };

    use std::cmp::min;
    use std::pin::Pin;

    // Starting a real TCP stream and opening a port would be good for an integration test, but here we're going to write a unit test, because why let reality fk w your nonsense code. We'll make a mock TcpStream to test on. Mostly a copy paste of Tcp with just the functionality that we need: Read and Write.
    struct MockTcpStream {
        read_data: Vec<u8>,
        write_data: Vec<u8>,
    }

    impl async_std::io::Read for MockTcpStream {
        fn poll_read(
            self: Pin<&mut Self>,
            _: &mut Context,
            buf: &mut [u8],
        ) -> Poll<Result<usize, Error>> {
            let size: usize = min(self.read_data.len(), buf.len());
            buf[..size].copy_from_slice(&self.read_data[..size]);
            Poll::Ready(Ok(size))
        }
    }
    impl async_std::io::Write for MockTcpStream {
        fn poll_write(
            mut self: Pin<&mut Self>,
            _: &mut Context,
            buf: &[u8],
        ) -> Poll<Result<usize, Error>> {
            self.write_data = Vec::from(buf);

            Poll::Ready(Ok(buf.len()))
        }

        fn poll_flush(self: Pin<&mut Self>, _: &mut Context) -> Poll<Result<(), Error>> {
            Poll::Ready(Ok(()))
        }

        fn poll_close(self: Pin<&mut Self>, _: &mut Context) -> Poll<Result<(), Error>> {
            Poll::Ready(Ok(()))
        }
    }

    // Lastly, our mock will need to implement Unpin, signifying that its location in memory can safely be moved. For more information on pinning and the Unpin trait, see the section on pinning.
    use std::marker::Unpin;
    impl Unpin for MockTcpStream {}

    // to write an async test, we declare it slightly differently:
    #[async_std::test]
    async fn test_handle_connection() {
        // a mock of what the browser client would send to main, and main would send to handle_conn:
        use std::fs;
        let input_bytes = b"GET / HTTP/1.1\r\n";
        let mut contents = vec![0u8; 1024];
        contents[..input_bytes.len()].clone_from_slice(input_bytes);
        let mut stream = MockTcpStream {
            read_data: contents,
            write_data: Vec::new(),
        };
        handle_connection(&mut stream).await;
        let mut buf = [0u8; 1024];
        stream.read(&mut buf).await.unwrap();

        let expected_contents = fs::read_to_string("hello.html").unwrap();
        let expected_response = format!("HTTP/1.1 200 OK\r\n\r\n{}", expected_contents);
        assert!(stream.write_data.starts_with(expected_response.as_bytes()));
    }
}

// and that's it! ~~
