use std::io::Read;

use bytes::BufMut;

pub(crate) struct Reader {
    inner: std::rc::Rc<std::net::TcpStream>,
    buffer_pool: std::rc::Rc<crate::BufferPool>,
    session: std::rc::Rc<crate::Session>,
    decoder: mqtt3::proto::PacketDecoder,

    pending_packet: Option<mqtt3::proto::Packet>,
    pending_read: Option<bytes::BytesMut>,
}

impl Reader {
    pub(crate) fn new(
        inner: std::rc::Rc<std::net::TcpStream>,
        buffer_pool: std::rc::Rc<crate::BufferPool>,
        session: std::rc::Rc<crate::Session>,
    ) -> Self {
        Reader {
            inner,
            buffer_pool,
            session,
            decoder: Default::default(),

            pending_packet: None,
            pending_read: None,
        }
    }

    pub(crate) fn poll(&mut self, cx: &mut std::task::Context<'_>) -> std::task::Poll<std::io::Result<()>> {
        let fd = std::os::unix::io::AsRawFd::as_raw_fd(self);

        let buf =
            if let Some(buf) = &mut self.pending_read {
                buf
            }
            else {
                let buf = match self.buffer_pool.poll_take(cx) {
                    std::task::Poll::Ready(buf) => buf,
                    std::task::Poll::Pending => return std::task::Poll::Pending,
                };
                self.pending_read.get_or_insert(buf)
            };

        loop {
            match self.session.poll_recv_ready(cx, fd) {
                std::task::Poll::Ready(()) => (),
                std::task::Poll::Pending => return std::task::Poll::Pending,
            }

            if let Some(pending_packet) = self.pending_packet.take() {
                self.session.recv(cx, fd, pending_packet)?;
            }

            match mqtt3::proto::decode(&mut self.decoder, buf).map_err(|err| std::io::Error::new(std::io::ErrorKind::Other, err))? {
                Some(packet) => self.pending_packet = Some(packet),
                None => {
                    let mut read_buf = as_read_buf(buf);

                    let read = match (&*self.inner).read(&mut read_buf) {
                        Ok(read) => read,
                        Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => return std::task::Poll::Pending,
                        Err(err) => return std::task::Poll::Ready(Err(err)),
                    };

                    if read == 0 {
                        return std::task::Poll::Ready(Err(std::io::ErrorKind::UnexpectedEof.into()));
                    }

                    unsafe { buf.advance_mut(read); }

                    eprintln!("fd {}: read {} bytes", fd, read);
                },
            }
        }
    }
}

impl std::fmt::Debug for Reader {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Reader")
            .field("inner", &std::os::unix::io::AsRawFd::as_raw_fd(self))
            .field("pending_read", &self.pending_read)
            .finish()
    }
}

impl std::os::unix::io::AsRawFd for Reader {
    fn as_raw_fd(&self) -> std::os::unix::io::RawFd {
        self.inner.as_raw_fd()
    }
}

fn as_read_buf(buf: &'_ mut bytes::BytesMut) -> &'_ mut [u8] {
    if !buf.has_remaining_mut() {
        buf.reserve(buf.capacity());
    }

    let chunk = buf.chunk_mut();

    // tokio converts the UninitSlice directly to [MaybeUninit<u8>] because UninitSlice is repr(transparent) over that type.
    // However this is actually an undocumented implementation detail of UninitSlice, let's not rely on it.
    // Instead we construct it manually from its as_mut_ptr() and len().

    let ptr: *mut std::mem::MaybeUninit<u8> = chunk.as_mut_ptr().cast();
    let len = chunk.len();
    unsafe {
        let read_buf = std::slice::from_raw_parts_mut(ptr, len);
        read_buf.fill(std::mem::MaybeUninit::new(0));

        // TODO: Use std::mem::MaybeUninit::slice_assume_init_mut when that is stabilized.
        let ptr: *mut u8 = read_buf.as_mut_ptr().cast();
        let read_buf = std::slice::from_raw_parts_mut(ptr, len);
        read_buf
    }
}
