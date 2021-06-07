// #![deny(rust_2018_idioms, warnings)]
#![deny(clippy::all, clippy::pedantic)]
#![allow(
)]

mod acceptor;
pub use acceptor::Acceptor;

mod buffer_pool;
pub use buffer_pool::BufferPool;

mod reader;
use reader::Reader;

mod runtime;
pub use runtime::Runtime;

mod session;
pub use session::Session;

mod writer;
use writer::Writer;

// struct SessionLayer {
// }

// impl SessionLayer {
//     fn poll(&mut self, _cx: &mut std::task::Context<'_>) -> std::task::Poll<()> {
//         std::task::Poll::Pending
//     }
// }
