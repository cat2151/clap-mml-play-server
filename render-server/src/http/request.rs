//! HTTP request の読み取り。`Content-Length` 必須・上限つきの最小実装。

use super::response::StatusCode;

const MAX_HEADER_BYTES: usize = 16 * 1024;

#[derive(Debug)]
pub(super) struct HttpRequest {
    pub(super) method: String,
    pub(super) path: String,
    pub(super) headers: Vec<(String, String)>,
    pub(super) body: Vec<u8>,
}

impl HttpRequest {
    pub(super) fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    }
}

#[derive(Debug)]
pub(super) struct RequestError {
    pub(super) status: StatusCode,
    pub(super) message: String,
}

impl RequestError {
    fn new(status: StatusCode, message: impl Into<String>) -> Self {
        Self {
            status,
            message: message.into(),
        }
    }
}

pub(super) fn read_request(
    reader: &mut impl std::io::Read,
    max_body_bytes: usize,
) -> Result<HttpRequest, RequestError> {
    let mut buffer = Vec::new();
    let mut scratch = [0u8; 4096];
    let header_end = loop {
        if let Some(header_end) = find_header_end(&buffer) {
            break header_end;
        }
        if buffer.len() >= MAX_HEADER_BYTES {
            return Err(RequestError::new(
                StatusCode::RequestHeaderFieldsTooLarge,
                "request headers are too large",
            ));
        }
        let read = reader.read(&mut scratch).map_err(|error| {
            RequestError::new(
                StatusCode::BadRequest,
                format!("failed to read request: {error}"),
            )
        })?;
        if read == 0 {
            return Err(RequestError::new(
                StatusCode::BadRequest,
                "request ended before headers were complete",
            ));
        }
        buffer.extend_from_slice(&scratch[..read]);
    };

    let head = std::str::from_utf8(&buffer[..header_end - 4]).map_err(|_| {
        RequestError::new(
            StatusCode::BadRequest,
            "request headers must be valid UTF-8",
        )
    })?;
    let mut lines = head.split("\r\n");
    let request_line = lines
        .next()
        .ok_or_else(|| RequestError::new(StatusCode::BadRequest, "missing request line"))?;
    let mut parts = request_line.split_whitespace();
    let method = parts
        .next()
        .ok_or_else(|| RequestError::new(StatusCode::BadRequest, "missing request method"))?;
    let path = parts
        .next()
        .ok_or_else(|| RequestError::new(StatusCode::BadRequest, "missing request path"))?;
    let _version = parts
        .next()
        .ok_or_else(|| RequestError::new(StatusCode::BadRequest, "missing HTTP version"))?;
    if parts.next().is_some() {
        return Err(RequestError::new(
            StatusCode::BadRequest,
            "malformed request line",
        ));
    }

    let mut headers = Vec::new();
    for line in lines {
        let Some((name, value)) = line.split_once(':') else {
            return Err(RequestError::new(
                StatusCode::BadRequest,
                "malformed request header",
            ));
        };
        headers.push((name.trim().to_ascii_lowercase(), value.trim().to_string()));
    }

    let content_length = headers
        .iter()
        .find(|(name, _)| name == "content-length")
        .map(|(_, value)| {
            value
                .parse::<usize>()
                .map_err(|_| RequestError::new(StatusCode::BadRequest, "invalid Content-Length"))
        })
        .transpose()?
        .ok_or_else(|| RequestError::new(StatusCode::LengthRequired, "Content-Length required"))?;
    if content_length > max_body_bytes {
        return Err(RequestError::new(
            StatusCode::PayloadTooLarge,
            format!("request body is too large; limit is {max_body_bytes} bytes"),
        ));
    }

    let mut body = buffer[header_end..].to_vec();
    if body.len() > content_length {
        body.truncate(content_length);
    }
    while body.len() < content_length {
        let read = reader.read(&mut scratch).map_err(|error| {
            RequestError::new(
                StatusCode::BadRequest,
                format!("failed to read request body: {error}"),
            )
        })?;
        if read == 0 {
            return Err(RequestError::new(
                StatusCode::BadRequest,
                "request ended before body was complete",
            ));
        }
        let remaining = content_length - body.len();
        body.extend_from_slice(&scratch[..read.min(remaining)]);
    }

    Ok(HttpRequest {
        method: method.to_string(),
        path: path.to_string(),
        headers,
        body,
    })
}

fn find_header_end(bytes: &[u8]) -> Option<usize> {
    bytes
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .map(|index| index + 4)
}
