use crate::io_uring::SubmissionQueueEventExt as _;
use anyhow::Context as _;
use iou::IoUring;
use std::{
    net::{TcpListener, TcpStream, ToSocketAddrs},
    os::unix::prelude::*,
    ptr, slice,
};

const QUEUE_DEPTH: u32 = 256;
const READ_SIZE: usize = 8096;

enum UserData {
    Accept,
    Read(Box<ReadData>),
    Write(Box<WriteData>),
}

struct ReadData {
    #[allow(dead_code)]
    client_socket: Option<TcpStream>,
    iovecs: Vec<libc::iovec>,
}

impl Drop for ReadData {
    fn drop(&mut self) {
        for iovec in self.iovecs.drain(..) {
            unsafe {
                libc::free(iovec.iov_base as _);
            }
        }
    }
}

struct WriteData {
    #[allow(dead_code)]
    client_socket: TcpStream,
    iovecs: Vec<libc::iovec>,
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

            match *user_data {
                UserData::Accept => {
                    println!("accept");
                    self.add_accept_request()?;
                    let client_socket = unsafe { TcpStream::from_raw_fd(res as _) };
                    self.add_read_request(client_socket)?;
                }

                UserData::Read(..) if res == 0 => {
                    eprintln!("warning: empty request");
                }
                UserData::Read(mut user_data) => {
                    println!("read {} bytes", res);

                    // parse HTTP request.
                    let buf = &user_data.iovecs[0];
                    let buf = unsafe {
                        slice::from_raw_parts(
                            buf.iov_base as *const u8, //
                            buf.iov_len,
                        )
                    };
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
                        user_data.client_socket.take().unwrap(),
                        b"\
                            HTTP/1.1 404 Not Found\r\n\
                            Server: io-uring-http-server\r\n\
                            Content-Length: 0\r\n\
                            Date: Tue, 12 May 2020 09:44:32 GMT\r\n\
                            \r\n\
                        ",
                    )?;
                }

                UserData::Write(..) => {
                    // do nothing.
                }
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

        let user_data = Box::new(UserData::Accept);
        sqe.set_user_data(Box::into_raw(user_data) as _);

        self.io_uring.submit_sqes()?;

        Ok(())
    }

    fn add_read_request(&mut self, client_socket: TcpStream) -> anyhow::Result<()> {
        let mut sqe = self
            .io_uring
            .next_sqe()
            .context("submission queue is full")?;

        let client_socket_fd = client_socket.as_raw_fd();
        let mut user_data = Box::new(ReadData {
            client_socket: Some(client_socket),
            iovecs: vec![libc::iovec {
                iov_base: unsafe { libc::calloc(READ_SIZE, 1) },
                iov_len: READ_SIZE,
            }],
        });
        unsafe {
            sqe.prep_readv(client_socket_fd, &mut user_data.iovecs[..], 0);
        }

        let user_data = Box::new(UserData::Read(user_data));
        sqe.set_user_data(Box::into_raw(user_data) as _);

        self.io_uring.submit_sqes()?;

        Ok(())
    }

    fn add_write_request(
        &mut self,
        client_socket: TcpStream,
        buf: &'static [u8],
    ) -> anyhow::Result<()> {
        let mut sqe = self
            .io_uring
            .next_sqe()
            .context("submission queue is full")?;

        let client_socket_fd = client_socket.as_raw_fd();
        let user_data = Box::new(WriteData {
            client_socket,
            iovecs: vec![libc::iovec {
                iov_base: buf.as_ptr() as *mut _,
                iov_len: buf.len(),
            }],
        });
        unsafe {
            sqe.prep_writev(client_socket_fd, &user_data.iovecs[..], 0);
        }

        let user_data = Box::new(UserData::Write(user_data));
        sqe.set_user_data(Box::into_raw(user_data) as _);

        self.io_uring.submit_sqes()?;

        Ok(())
    }
}
