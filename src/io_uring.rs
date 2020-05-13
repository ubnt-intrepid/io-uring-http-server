use std::{
    io,
    net::{TcpListener, TcpStream},
    os::unix::prelude::*,
};

pub enum EventType {
    Accept,
    Read,
    Write,
}

#[allow(dead_code)]
pub struct Event {
    pub event_type: EventType,
    pub client_socket: Option<TcpStream>,
    pub buf: Vec<u8>,
}

pub struct Uring(iou::IoUring);

impl Uring {
    pub fn new(entries: u32) -> io::Result<Self> {
        iou::IoUring::new(entries).map(Self)
    }

    pub fn wait_cqe(&mut self) -> io::Result<(Box<Event>, io::Result<usize>)> {
        let cqe = self.0.wait_for_cqe()?;

        let user_data = unsafe {
            let ptr = cqe.user_data() as *mut Event;
            Box::from_raw(ptr)
        };

        let res = cqe.result();

        Ok((user_data, res))
    }

    pub fn next_sqe(&mut self) -> io::Result<SubmissionEvent<'_>> {
        self.0.next_sqe().map(SubmissionEvent).ok_or_else(|| {
            io::Error::new(io::ErrorKind::Other, "io_uring submission queue is empty")
        })
    }

    pub fn submit_sqes(&mut self) -> io::Result<()> {
        self.0.submit_sqes()?;
        Ok(())
    }
}

pub struct SubmissionEvent<'a>(iou::SubmissionQueueEvent<'a>);

impl SubmissionEvent<'_> {
    pub fn accept(mut self, listener: &TcpListener) -> io::Result<()> {
        let sqe = &mut self.0;

        unsafe {
            sqe.prep_accept(listener.as_raw_fd(), None, iou::SockFlag::empty());
        }

        let user_data = Box::new(Event {
            event_type: EventType::Accept,
            client_socket: None,
            buf: Vec::new(),
        });
        sqe.set_user_data(Box::into_raw(user_data) as _);

        Ok(())
    }

    pub fn read(mut self, client_socket: TcpStream, mut buf: Vec<u8>) -> anyhow::Result<()> {
        let sqe = &mut self.0;

        unsafe {
            sqe.prep_read(client_socket.as_raw_fd(), &mut buf[..], 0);
        }

        let user_data = Box::new(Event {
            event_type: EventType::Read,
            client_socket: Some(client_socket),
            buf,
        });
        sqe.set_user_data(Box::into_raw(user_data) as _);

        Ok(())
    }

    pub fn write(mut self, client_socket: TcpStream, buf: Vec<u8>) -> io::Result<()> {
        let sqe = &mut self.0;

        unsafe {
            sqe.prep_write(client_socket.as_raw_fd(), &buf[..], 0);
        }

        let user_data = Box::new(Event {
            event_type: EventType::Write,
            client_socket: Some(client_socket),
            buf,
        });
        sqe.set_user_data(Box::into_raw(user_data) as _);

        Ok(())
    }
}
