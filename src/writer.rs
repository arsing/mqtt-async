use std::io::Write;

pub(crate) struct Writer {
    inner: std::rc::Rc<std::net::TcpStream>,
}

impl Writer {
    pub(crate) fn new(inner: std::rc::Rc<std::net::TcpStream>) -> Self {
        Writer {
            inner,
        }
    }

    pub(crate) fn poll(&mut self, _cx: &mut std::task::Context<'_>, buf: &mut bytes::BytesMut) -> std::task::Poll<std::io::Result<()>> {
        match (&*self.inner).write(&buf) {
            Ok(written) => {
                bytes::Buf::advance(buf, written);
                std::task::Poll::Ready(Ok(()))
            },
            Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => std::task::Poll::Pending,
            Err(err) => std::task::Poll::Ready(Err(err)),
        }
    }
}

impl std::os::unix::io::AsRawFd for Writer {
    fn as_raw_fd(&self) -> std::os::unix::io::RawFd {
        self.inner.as_raw_fd()
    }
}
