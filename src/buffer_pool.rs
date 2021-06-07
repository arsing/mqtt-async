pub struct BufferPool {
    inner: std::cell::RefCell<BufferPoolInner>,
}

struct BufferPoolInner {
    pool: std::collections::VecDeque<bytes::BytesMut>,
    wakers: std::collections::VecDeque<std::task::Waker>,
}

const CAPACITY: usize = 128;

impl BufferPool {
    pub fn new() -> std::rc::Rc<Self> {
        std::rc::Rc::new(BufferPool {
            inner: std::cell::RefCell::new(BufferPoolInner {
                pool: {
                    let mut pool = std::collections::VecDeque::with_capacity(CAPACITY);
                    for _ in 0..CAPACITY {
                        pool.push_back(bytes::BytesMut::with_capacity(8192));
                    }
                    pool
                },
                wakers: Default::default(),
            }),
        })
    }

    pub(crate) fn poll_take(&self, cx: &mut std::task::Context<'_>) -> std::task::Poll<bytes::BytesMut> {
        let mut inner = self.inner.borrow_mut();
        inner.poll_take(cx)
    }

    pub(crate) fn put_back(&self, buf: bytes::BytesMut) {
        let mut inner = self.inner.borrow_mut();
        inner.put_back(buf)
    }
}

impl BufferPoolInner {
    fn poll_take(&mut self, cx: &mut std::task::Context<'_>) -> std::task::Poll<bytes::BytesMut> {
        if let Some(buf) = self.pool.pop_front() {
            eprintln!("BufferPool::poll_take Ok");
            std::task::Poll::Ready(buf)
        }
        else {
            eprintln!("BufferPool::poll_take Pending");
            self.wakers.push_back(cx.waker().clone());
            std::task::Poll::Pending
        }
    }

    fn put_back(&mut self, mut buf: bytes::BytesMut) {
        eprintln!("BufferPool::put_back");
        buf.clear();
        self.pool.push_back(buf);
        if let Some(waker) = self.wakers.pop_front() {
            waker.wake();
        }
    }
}
