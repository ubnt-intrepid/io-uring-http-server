mod io_uring;
mod request;

use crate::{
    io_uring::{EventType, Uring},
    request::Request,
};
use anyhow::Context as _;
use std::{
    fs, io,
    net::{TcpListener, TcpStream},
    os::unix::prelude::*,
    path::Path,
};

const QUEUE_DEPTH: u32 = 256;
const READ_SIZE: usize = 8096;

const MIN_KERNEL_MAJOR_VERSION: u16 = 5;
const MIN_KERNEL_MINOR_VERSION: u16 = 5;

fn main() -> anyhow::Result<()> {
    let (major, minor) = get_kernel_version().context("failed to check the kernel version")?;
    anyhow::ensure!(
        major >= MIN_KERNEL_MAJOR_VERSION && minor >= MIN_KERNEL_MINOR_VERSION,
        "The kernel version must be at least {}.{} (your version is {}.{})",
        MIN_KERNEL_MAJOR_VERSION,
        MIN_KERNEL_MINOR_VERSION,
        major,
        minor,
    );

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

                let response_msg = handle_request(request)
                    .unwrap_or_else(|_err| make_error_response_msg("500 Internal Server Error"));

                uring
                    .next_sqe()?
                    .write(user_data.client_socket.unwrap(), response_msg)?;
            }

            EventType::Write => {
                log::trace!("--> write {} bytes", n);
                /* do nothing. */
            }
        }

        uring.submit_sqes()?;
    }
}

fn handle_request(request: Request<'_>) -> anyhow::Result<Vec<u8>> {
    match request.method {
        "GET" => {
            // FIXME: asyncify

            let is_dir = request.path.ends_with('/');
            let mut path = Path::new("public").join(&request.path[1..]);
            if is_dir {
                path.push("index.html");
            }

            let file = match fs::OpenOptions::new().read(true).open(&path) {
                Ok(f) => f,
                Err(err) if err.kind() == io::ErrorKind::NotFound => {
                    return Ok(make_error_response_msg("404 Not Found"));
                }
                Err(err) => anyhow::bail!(err),
            };

            let metadata = file.metadata()?;
            let mut content = io::BufReader::new(file);

            let content_type = match path.extension().and_then(|ext| ext.to_str()) {
                Some("jpg") | Some("jpeg") => "image/jpg",
                Some("png") => "image/png",
                Some("gif") => "image/gif",
                Some("html") | Some("htm") => "text/html",
                Some("js") => "application/javascript",
                Some("css") => "text/css",
                Some("txt") => "text/plain",
                Some("json") => "application/json",
                _ => "text/plain",
            };
            let content_length = metadata.len();

            let mut response_msg = vec![0u8; 0];
            use std::io::Write as _;
            write!(
                &mut response_msg,
                "\
                    HTTP/1.1 200 OK\r\n\
                    Content-Type: {content_type}\r\n\
                    Content-Length: {content_length}\r\n\
                    \r\n\
                ",
                content_type = content_type,
                content_length = content_length,
            )?;

            io::copy(&mut content, &mut response_msg)?;

            Ok(response_msg)
        }
        _ => Ok(make_error_response_msg("400 Bad Request")),
    }
}

fn make_error_response_msg(status: &str) -> Vec<u8> {
    format!(
        "\
            HTTP/1.1 {status}\r\n\
            Content-type: text/html\r\n\
            \r\n\
            <html>\
            <head>\
            <title>{status}</title>\
            </head>\
            <body>\
            <h1>{status}</h1>\
            </body>\
            </html>\
        ",
        status = status
    )
    .into()
}

fn get_kernel_version() -> anyhow::Result<(u16, u16)> {
    use std::ffi::CStr;

    let uname = unsafe {
        let mut buf = std::mem::MaybeUninit::<libc::utsname>::uninit();

        let rc = libc::uname(buf.as_mut_ptr());
        if rc < 0 {
            return Err(io::Error::last_os_error()).context("uname");
        }

        buf.assume_init()
    };

    let release: &CStr = unsafe { CStr::from_ptr(uname.release.as_ptr()) };
    let release = release.to_str()?;

    let mut iter = release.split('.');
    let major = iter.next().context("missing major")?.parse()?;
    let minor = iter.next().context("missing minor")?.parse()?;

    Ok((major, minor))
}
