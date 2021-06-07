pub struct Runtime {
    acceptor: crate::Acceptor,
    acceptor_fd: std::os::unix::io::RawFd,
    session: std::rc::Rc<crate::Session>,
    readers: std::collections::BTreeMap<std::os::unix::io::RawFd, crate::Reader>,

    epoll_fd: std::os::unix::io::RawFd,
    pending_wake_fd: std::os::unix::io::RawFd,
    pending_wakes: std::rc::Rc<std::cell::RefCell<std::collections::BTreeSet<std::os::unix::io::RawFd>>>,
}

impl Runtime {
    pub fn new(acceptor: crate::Acceptor, session: std::rc::Rc<crate::Session>) -> nix::Result<Self> {
        let acceptor_fd = std::os::unix::io::AsRawFd::as_raw_fd(&acceptor);

        let epoll_fd = nix::sys::epoll::epoll_create1(nix::sys::epoll::EpollCreateFlags::EPOLL_CLOEXEC)?;
        let pending_wake_fd = nix::sys::eventfd::eventfd(0, nix::sys::eventfd::EfdFlags::EFD_CLOEXEC | nix::sys::eventfd::EfdFlags::EFD_NONBLOCK)?;

        let () = nix::sys::epoll::epoll_ctl(
            epoll_fd,
            nix::sys::epoll::EpollOp::EpollCtlAdd,
            acceptor_fd,
            Some(&mut nix::sys::epoll::EpollEvent::new(
                nix::sys::epoll::EpollFlags::EPOLLIN | nix::sys::epoll::EpollFlags::EPOLLET,
                acceptor_fd as _,
            )),
        )?;

        Ok(Runtime {
            acceptor,
            acceptor_fd,
            session,
            readers: Default::default(),

            epoll_fd,
            pending_wake_fd,
            pending_wakes: Default::default(),
        })
    }

    pub fn run(mut self) -> nix::Result<()> {
        let mut ready: std::collections::BTreeMap<_, _> = Default::default();

        loop {
            let mut events = [nix::sys::epoll::EpollEvent::empty(); 1024];
            let num_events = nix::sys::epoll::epoll_wait(self.epoll_fd, &mut events, -1)?;
            let events = &mut events[..num_events];

            for event in events {
                let fd = event.data() as std::os::unix::io::RawFd;

                if fd == self.pending_wake_fd {
                    let mut pending_wakes = self.pending_wakes.try_borrow_mut().expect("runtime could not lock pending_wakes mutex");
                    for &fd in &*pending_wakes {
                        *ready.entry(fd).or_insert_with(nix::sys::epoll::EpollFlags::empty) |=
                            nix::sys::epoll::EpollFlags::EPOLLIN | nix::sys::epoll::EpollFlags::EPOLLOUT;
                    }
                    pending_wakes.clear();
                }
                else {
                    *ready.entry(fd).or_insert_with(nix::sys::epoll::EpollFlags::empty) |= event.events();
                }
            }

            for (&fd, &flags) in &ready {
                let waker = new_waker(fd, self.pending_wakes.clone(), self.pending_wake_fd);
                let mut cx = std::task::Context::from_waker(&waker);

                if fd == self.acceptor_fd {
                    assert_eq!(flags, nix::sys::epoll::EpollFlags::EPOLLIN);
                    match self.acceptor.poll(&mut cx) {
                        std::task::Poll::Ready(Ok(reader)) => {
                            register_reader(self.epoll_fd, &mut self.readers, reader)?;
                        },
                        std::task::Poll::Ready(Err(err)) => {
                            eprintln!("Acceptor fd {} had err {}", fd, err);
                        },
                        std::task::Poll::Pending => (),
                    }
                }
                else if let Some(reader) = self.readers.get_mut(&fd) {
                    if flags.contains(nix::sys::epoll::EpollFlags::EPOLLIN) {
                        match reader.poll(&mut cx) {
                            std::task::Poll::Ready(Ok(())) => (),
                            std::task::Poll::Ready(Err(err)) => {
                                eprintln!("Reader fd {} had err {}", fd, err);
                                unregister_reader(self.epoll_fd, &mut self.readers, fd)?;
                            },
                            std::task::Poll::Pending => (),
                        }
                    }
                    else if flags.contains(nix::sys::epoll::EpollFlags::EPOLLOUT) {
                        match self.session.poll_write(&mut cx, fd) {
                            std::task::Poll::Ready(Ok(())) => (),
                            std::task::Poll::Ready(Err(err)) => {
                                eprintln!("Writer fd {} had err {}", fd, err);
                                unregister_reader(self.epoll_fd, &mut self.readers, fd)?;
                            },
                            std::task::Poll::Pending => (),
                        }
                    }
                    else {
                        panic!("Reader fd {} became ready but for flags {:?}", fd, flags);
                    }
                }
                else {
                    panic!("runtime received event for fd {} that isn't the acceptor or any reader", fd);
                }
            }

            ready.clear();
        }
    }
}

#[derive(Clone)]
struct Handle {
}

fn register_reader(
    epoll_fd: std::os::unix::io::RawFd,
    readers: &mut std::collections::BTreeMap<std::os::unix::io::RawFd, crate::Reader>,
    reader: crate::Reader,
) -> nix::Result<()> {
    let reader_fd = std::os::unix::io::AsRawFd::as_raw_fd(&reader);
    let () = nix::sys::epoll::epoll_ctl(
        epoll_fd,
        nix::sys::epoll::EpollOp::EpollCtlAdd,
        reader_fd,
        Some(&mut nix::sys::epoll::EpollEvent::new(
            nix::sys::epoll::EpollFlags::EPOLLIN | nix::sys::epoll::EpollFlags::EPOLLOUT | nix::sys::epoll::EpollFlags::EPOLLET,
            reader_fd as _,
        )),
    )?;
    readers.insert(reader_fd, reader);
    Ok(())
}

fn unregister_reader(
    epoll_fd: std::os::unix::io::RawFd,
    readers: &mut std::collections::BTreeMap<std::os::unix::io::RawFd, crate::Reader>,
    reader_fd: std::os::unix::io::RawFd,
) -> nix::Result<()> {
    let () = nix::sys::epoll::epoll_ctl(
        epoll_fd,
        nix::sys::epoll::EpollOp::EpollCtlDel,
        reader_fd,
        None,
    )?;
    readers.remove(&reader_fd);
    Ok(())
}

fn new_waker(
    fd: std::os::unix::io::RawFd,
    pending_wakes: std::rc::Rc<std::cell::RefCell<std::collections::BTreeSet<std::os::unix::io::RawFd>>>,
    pending_wake_fd: std::os::unix::io::RawFd,
) -> std::task::Waker {
    struct Waker {
        fd: std::os::unix::io::RawFd,
        pending_wakes: std::rc::Rc<std::cell::RefCell<std::collections::BTreeSet<std::os::unix::io::RawFd>>>,
        pending_wake_fd: std::os::unix::io::RawFd,
    }

    impl Waker {
        fn into_raw_waker(self: std::rc::Rc<Self>) -> std::task::RawWaker {
            std::task::RawWaker::new(std::rc::Rc::into_raw(self).cast(), &RAW_WAKER_VTABLE)
        }

        fn wake(self: std::rc::Rc<Self>) {
            self.wake_by_ref();
        }

        fn wake_by_ref(self: &std::rc::Rc<Self>) {
            let mut pending_wakes =
                self.pending_wakes.try_borrow_mut()
                .map_err(|err| format!("waker {} could not lock pending_wakes mutex: {}", self.fd, err))
                .unwrap();
            pending_wakes.insert(self.fd);
            let written =
                nix::unistd::write(self.pending_wake_fd, &(1_u64.to_ne_bytes()))
                .map_err(|err| format!("waker {} could not write to pending_wake_fd: {}", self.fd, err))
                .unwrap();
            assert_eq!(written, 8, "waker {} could not write to pending_wake_fd: short write of {} bytes", self.fd, written);
        }
    }

    const RAW_WAKER_VTABLE: std::task::RawWakerVTable = std::task::RawWakerVTable::new(
        raw_waker_clone,
        raw_waker_wake,
        raw_waker_wake_by_ref,
        raw_waker_drop,
    );

    unsafe fn raw_waker_clone(data: *const ()) -> std::task::RawWaker {
        let waker: *const Waker = data.cast();
        // Wrap in ManuallyDrop so that it isn't dropped.
        // We don't want to drop it because raw_waker_clone receives &Self, not Self.
        let waker = std::mem::ManuallyDrop::new(std::rc::Rc::from_raw(waker));
        let result = std::rc::Rc::clone(&*waker);
        result.into_raw_waker()
    }

    unsafe fn raw_waker_wake(data: *const ()) {
        let waker: *const Waker = data.cast();
        let waker = std::rc::Rc::from_raw(waker);
        waker.wake()
    }

    unsafe fn raw_waker_wake_by_ref(data: *const ()) {
        let waker: *const Waker = data.cast();
        // Wrap in ManuallyDrop so that it isn't dropped.
        // We don't want to drop it because raw_waker_wake_by_ref receives &Self, not Self.
        let waker = std::mem::ManuallyDrop::new(std::rc::Rc::from_raw(waker));
        waker.wake_by_ref()
    }

    unsafe fn raw_waker_drop(data: *const ()) {
        let waker: *const Waker = data.cast();
        let waker = std::rc::Rc::from_raw(waker);
        drop(waker)
    }

    let waker = std::rc::Rc::new(Waker {
        fd,
        pending_wakes,
        pending_wake_fd,
    });
    let raw_waker = waker.into_raw_waker();
    unsafe { std::task::Waker::from_raw(raw_waker) }
}
