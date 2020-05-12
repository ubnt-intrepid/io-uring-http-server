mod io_uring;
mod request;

use crate::io_uring::{EventType, Uring};
use anyhow::Context as _;
use std::{
    net::{TcpListener, TcpStream},
    os::unix::prelude::*,
};

const QUEUE_DEPTH: u32 = 256;
const READ_SIZE: usize = 8096;

const NOT_FOUND: &str = "\
    HTTP/1.0 404 Not Found\r\n\
    Content-type: text/html\r\n\
    \r\n\
    <html>\
    <head>\
    <title>404 Not Found</title>\
    </head>\
    <body>\
    <h1>404 Not Found</h1>\
    </body>\
    </html>\
";

const BAD_REQUEST: &str = "\
    HTTP/1.0 400 Bad Request\r\n\
    Content-type: text/html\r\n\
    \r\n\
    <html>\
    <head>\
    <title>400 Bad Request</title>\
    </head>\
    <body>\
    <h1>400 Bad Request</h1>\
    </body>\
    </html>\
";

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

                let request = crate::request::parse(buf)?
                    .ok_or_else(|| anyhow::anyhow!("unimplemented: continue read request"))?;
                log::info!("{} {}", request.method, request.path);

                match request.method {
                    "GET" => {
                        // TODO: handle HTTP methods
                        uring
                            .next_sqe()?
                            .write(user_data.client_socket.unwrap(), NOT_FOUND.into())?;
                    }
                    _ => {
                        uring
                            .next_sqe()?
                            .write(user_data.client_socket.unwrap(), BAD_REQUEST.into())?;
                    }
                }
            }

            EventType::Write => {
                log::trace!("--> write {} bytes", n);
                /* do nothing. */
            }
        }

        uring.submit_sqes()?;
    }
}
