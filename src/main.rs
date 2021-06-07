#![deny(rust_2018_idioms, warnings)]
#![deny(clippy::all, clippy::pedantic)]
#![allow(
)]

fn main() {
    let buffer_pool = mqtt_async::BufferPool::new();
    let session = mqtt_async::Session::new(buffer_pool);
    let acceptor = mqtt_async::Acceptor::bind(("::", 1883), session.clone()).unwrap();
    let runtime = mqtt_async::Runtime::new(acceptor, session).unwrap();
    let () = runtime.run().unwrap();
}
