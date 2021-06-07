pub struct Session {
    inner: std::cell::RefCell<SessionInner>,
}

struct SessionInner {
    buffer_pool: std::rc::Rc<crate::BufferPool>,
    clients: std::collections::BTreeMap<std::os::unix::io::RawFd, Client>,
}

struct Client {
    writer: crate::Writer,
    pending_packets: std::collections::VecDeque<mqtt3::proto::Packet>,
    pending_write: Option<bytes::BytesMut>,
}

impl Session {
    pub fn new(buffer_pool: std::rc::Rc<crate::BufferPool>) -> std::rc::Rc<Self> {
        std::rc::Rc::new(Session {
            inner: std::cell::RefCell::new(SessionInner {
                buffer_pool,
                clients: Default::default(),
            }),
        })
    }

    pub(crate) fn poll_accept_ready(&self, _cx: &mut std::task::Context<'_>) -> std::task::Poll<()> {
        let mut _inner = self.inner.borrow_mut();
        std::task::Poll::Ready(())
    }

    pub(crate) fn accept(self: std::rc::Rc<Self>, stream: std::net::TcpStream) -> std::io::Result<crate::Reader> {
        let mut inner = self.inner.borrow_mut();
        let inner = &mut *inner;

        let fd = std::os::unix::io::AsRawFd::as_raw_fd(&stream);
        eprintln!("accepting client {:?}", fd);

        stream.set_nonblocking(true)?;
        let stream = std::rc::Rc::new(stream);

        inner.clients.insert(fd, Client {
            writer: crate::Writer::new(stream.clone()),
            pending_packets: Default::default(),
            pending_write: None,
        });

        let reader = crate::Reader::new(stream, inner.buffer_pool.clone(), self.clone());
        Ok(reader)
    }

    pub(crate) fn poll_recv_ready(&self, _cx: &mut std::task::Context<'_>, _fd: std::os::unix::io::RawFd) -> std::task::Poll<()> {
        let mut _inner = self.inner.borrow_mut();
        std::task::Poll::Ready(())
    }

    pub(crate) fn recv(&self, cx: &mut std::task::Context<'_>, fd: std::os::unix::io::RawFd, packet: mqtt3::proto::Packet) -> std::io::Result<()> {
        let mut inner = self.inner.borrow_mut();
        eprintln!("fd {}: received {:?}", fd, packet);

        let Client { pending_packets, .. } =
            inner.clients.get_mut(&fd)
            .unwrap_or_else(|| panic!("session received recv for fd {} which is not associated with any Client", fd));

        match packet {
            mqtt3::proto::Packet::Connect(_) => {
                pending_packets.push_back(mqtt3::proto::Packet::ConnAck(mqtt3::proto::ConnAck {
                    session_present: false,
                    return_code: mqtt3::proto::ConnectReturnCode::Refused(mqtt3::proto::ConnectionRefusedReason::NotAuthorized),
                }));
            },

            _ => (),
        }

        // TODO:
        // Have to get the BytesMut used for packet back, and put_back() it into the BufferPool.
        // Do that with the new encoder. For now, just push a new BytesMut back.
        inner.buffer_pool.put_back(Default::default());

        match inner.poll_write(cx, fd) {
            std::task::Poll::Ready(result) => result,
            std::task::Poll::Pending => Ok(()),
        }
    }

    pub(crate) fn poll_write(&self, cx: &mut std::task::Context<'_>, fd: std::os::unix::io::RawFd) -> std::task::Poll<std::io::Result<()>> {
        let mut inner = self.inner.borrow_mut();
        inner.poll_write(cx, fd)
    }
}

impl SessionInner {
    fn poll_write(&mut self, cx: &mut std::task::Context<'_>, fd: std::os::unix::io::RawFd) -> std::task::Poll<std::io::Result<()>> {
        let Client { writer, pending_packets, pending_write } =
            self.clients.get_mut(&fd)
            .unwrap_or_else(|| panic!("session received poll_write for fd {} which is not associated with any Client", fd));

        loop {
            if let Some(mut buf) = pending_write.take() {
                while !buf.is_empty() {
                    match writer.poll(cx, &mut buf)? {
                        std::task::Poll::Ready(()) => (),
                        std::task::Poll::Pending => {
                            *pending_write = Some(buf);
                            return std::task::Poll::Pending;
                        },
                    }
                }

                self.buffer_pool.put_back(buf);
            }

            if let Some(packet) = pending_packets.pop_front() {
                match self.buffer_pool.poll_take(cx) {
                    std::task::Poll::Ready(mut buf) => {
                        mqtt3::proto::encode(packet, &mut buf).map_err(|err| std::io::Error::new(std::io::ErrorKind::Other, err))?;
                        *pending_write = Some(buf);
                    },
                    std::task::Poll::Pending => {
                        pending_packets.push_front(packet);
                        return std::task::Poll::Pending;
                    },
                }
            }
            else {
                break;
            }
        }

        std::task::Poll::Ready(Ok(()))
    }
}
