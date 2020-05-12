use crate::io_uring::SubmissionQueueEventExt as _;
use anyhow::Context as _;
use iou::IoUring;
use std::{
    net::{TcpListener, TcpStream, ToSocketAddrs},
    os::unix::prelude::*,
    ptr,
};

const QUEUE_DEPTH: u32 = 256;
const READ_SIZE: usize = 8096;

enum EventType {
    Accept,
    Read,
    Write,
}

#[allow(dead_code)]
struct UserData {
    event_type: EventType,
    client_socket: Option<TcpStream>,
    buf: Vec<u8>,
    iovecs: Option<Box<[libc::iovec; 1]>>,
}

pub struct Server {
    listener: TcpListener,
    io_uring: IoUring,
}

impl Server {
    pub fn bind<A: ToSocketAddrs>(addr: A) -> anyhow::Result<Self> {
        let listener = TcpListener::bind(addr) //
            .context("failed to open listener socket")?;

        let io_uring = IoUring::new(QUEUE_DEPTH) //
            .context("failed to init io_uring")?;

        Ok(Self { listener, io_uring })
    }

    pub fn run_loop(&mut self) -> anyhow::Result<()> {
        self.add_accept_request()?;

        loop {
            let (user_data, res);
            {
                let cqe = self
                    .io_uring
                    .wait_for_cqe()
                    .context("IoUring::wait_for_cqe")?;

                user_data = unsafe {
                    let ptr = cqe.user_data() as *mut UserData;
                    Box::from_raw(ptr)
                };

                res = cqe.result().context("async request failed")?;
            }

            match user_data.event_type {
                EventType::Accept => {
                    println!("accept");
                    self.add_accept_request()?;

                    let client_socket = unsafe { TcpStream::from_raw_fd(res as _) };
                    self.add_read_request(client_socket)?;
                }

                EventType::Read if res == 0 => eprintln!("warning: empty request"),
                EventType::Read => {
                    println!("read {} bytes", res);
                    let buf = &user_data.buf[..res];

                    // parse HTTP request.
                    let mut headers = [httparse::EMPTY_HEADER; 16];
                    let mut req = httparse::Request::new(&mut headers);
                    let status = req
                        .parse(buf) //
                        .context("failed to parse http request")?;
                    let _amt = match status {
                        httparse::Status::Complete(amt) => amt,
                        httparse::Status::Partial => {
                            anyhow::bail!("unimplemented: continue read request")
                        }
                    };

                    // TODO: handle HTTP methods
                    self.add_write_request(
                        user_data.client_socket.unwrap(),
                        "\
                            HTTP/1.1 404 Not Found\r\n\
                            Server: io-uring-http-server\r\n\
                            Content-Length: 0\r\n\
                            Date: Tue, 12 May 2020 09:44:32 GMT\r\n\
                            \r\n\
                        "
                        .into(),
                    )?;
                }

                EventType::Write => { /* do nothing. */ }
            }
        }
    }

    fn add_accept_request(&mut self) -> anyhow::Result<()> {
        let mut sqe = self
            .io_uring
            .next_sqe()
            .context("submission queue is full")?;

        unsafe {
            sqe.prep_accept(
                self.listener.as_raw_fd(),
                ptr::null_mut(),
                ptr::null_mut(),
                0,
            );
        }

        let user_data = Box::new(UserData {
            event_type: EventType::Accept,
            client_socket: None,
            buf: Vec::new(),
            iovecs: None,
        });
        sqe.set_user_data(Box::into_raw(user_data) as _);

        self.io_uring.submit_sqes()?;

        Ok(())
    }

    fn add_read_request(&mut self, client_socket: TcpStream) -> anyhow::Result<()> {
        let mut sqe = self
            .io_uring
            .next_sqe()
            .context("submission queue is full")?;

        let mut buf = vec![0u8; READ_SIZE];
        let mut iovecs = Box::new([libc::iovec {
            iov_base: buf.as_mut_ptr().cast(),
            iov_len: buf.len(),
        }]);
        unsafe {
            sqe.prep_readv(client_socket.as_raw_fd(), &mut iovecs[..], 0);
        }

        let user_data = Box::new(UserData {
            event_type: EventType::Read,
            client_socket: Some(client_socket),
            buf,
            iovecs: Some(iovecs),
        });
        sqe.set_user_data(Box::into_raw(user_data) as _);

        self.io_uring.submit_sqes()?;

        Ok(())
    }

    fn add_write_request(&mut self, client_socket: TcpStream, buf: Vec<u8>) -> anyhow::Result<()> {
        let mut sqe = self
            .io_uring
            .next_sqe()
            .context("submission queue is full")?;

        let iovecs = Box::new([libc::iovec {
            iov_base: buf.as_ptr() as *mut _,
            iov_len: buf.len(),
        }]);
        unsafe {
            sqe.prep_writev(client_socket.as_raw_fd(), &iovecs[..], 0);
        }

        let user_data = Box::new(UserData {
            event_type: EventType::Write,
            client_socket: Some(client_socket),
            buf,
            iovecs: Some(iovecs),
        });
        sqe.set_user_data(Box::into_raw(user_data) as _);

        self.io_uring.submit_sqes()?;

        Ok(())
    }
}
