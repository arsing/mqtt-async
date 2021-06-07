pub struct Acceptor {
    inner: std::net::TcpListener,
    session: std::rc::Rc<crate::Session>,
}

impl Acceptor {
    pub fn bind(
        addr: impl std::net::ToSocketAddrs,
        session: std::rc::Rc<crate::Session>,
    ) -> std::io::Result<Self> {
        let inner = std::net::TcpListener::bind(addr)?;
        inner.set_nonblocking(true)?;
        Ok(Acceptor {
            inner,
            session,
        })
    }

    pub(crate) fn poll(&mut self, cx: &mut std::task::Context<'_>) -> std::task::Poll<std::io::Result<crate::Reader>> {
        match self.session.poll_accept_ready(cx) {
            std::task::Poll::Ready(()) => (),
            std::task::Poll::Pending => return std::task::Poll::Pending,
        }

        match self.inner.accept() {
            Ok((stream, _)) => {
                let reader = self.session.clone().accept(stream)?;
                std::task::Poll::Ready(Ok(reader))
            },

            Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => std::task::Poll::Pending,

            Err(err) => std::task::Poll::Ready(Err(err)),
        }
    }
}

impl std::os::unix::io::AsRawFd for Acceptor {
    fn as_raw_fd(&self) -> std::os::unix::io::RawFd {
        self.inner.as_raw_fd()
    }
}
