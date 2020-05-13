use anyhow::Context as _;

pub struct Request<'a> {
    pub method: &'a str,
    pub path: &'a str,
    pub version: u8,
    pub headers: Vec<(&'a str, &'a [u8])>,
}

pub fn parse_request(buf: &[u8]) -> anyhow::Result<Option<Request<'_>>> {
    let mut headers = [httparse::EMPTY_HEADER; 16];

    let mut req = httparse::Request::new(&mut headers);
    let _amt = match req.parse(buf)? {
        httparse::Status::Complete(amt) => amt,
        httparse::Status::Partial => return Ok(None),
    };

    let method = req.method.context("missing HTTP method")?;
    let path = req.path.context("missing HTTP path")?;
    let version = req.version.context("missing HTTP version")?;

    let headers = req.headers.iter().map(|h| (h.name, h.value)).collect();

    Ok(Some(Request {
        method,
        path,
        version,
        headers,
    }))
}
