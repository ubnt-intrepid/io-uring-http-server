mod io_uring;

use crate::io_uring::{EventType, Uring};
use anyhow::Context as _;
use std::{
    net::{TcpListener, TcpStream},
    os::unix::prelude::*,
};

const QUEUE_DEPTH: u32 = 256;
const READ_SIZE: usize = 8096;

fn main() -> anyhow::Result<()> {
    pretty_env_logger::try_init()?;

    let listener = TcpListener::bind("127.0.0.1:8000") //
        .context("failed to open listener socket")?;

    if let Ok(local_addr) = listener.local_addr() {
        log::info!("Listening on {}", local_addr);
    }

    let mut uring = Uring::new(QUEUE_DEPTH) //
        .context("failed to init io_uring")?;
    uring.next_sqe()?.accept(&listener)?;
    uring.submit_sqes()?;

    loop {
        let (user_data, res) = uring.wait_cqe()?;
        let n = res.context("async request failed")?;

        log::trace!("receive a completion event");
        match user_data.event_type {
            EventType::Accept => {
                log::trace!("--> accept");
                uring.next_sqe()?.accept(&listener)?;

                let client_socket = unsafe { TcpStream::from_raw_fd(n as _) };
                let buf = vec![0u8; READ_SIZE];
                uring.next_sqe()?.read(client_socket, buf)?;
            }

            EventType::Read if n == 0 => eprintln!("warning: empty request"),
            EventType::Read => {
                log::trace!("--> read {} bytes", n);
                let buf = &user_data.buf[..n];

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

                let method = req.method.expect("missing HTTP method");
                let path = req.path.expect("missing HTTP path");
                log::info!("{} {}", method, path);

                // TODO: handle HTTP methods
                uring.next_sqe()?.write(
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

            EventType::Write => {
                log::trace!("--> write {} bytes", n);
                /* do nothing. */
            }
        }

        uring.submit_sqes()?;
    }
}
