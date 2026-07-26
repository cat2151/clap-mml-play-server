use anyhow::Result;

#[derive(Clone, Copy, Debug)]
pub(super) enum StatusCode {
    Ok,
    Accepted,
    NoContent,
    BadRequest,
    NotFound,
    MethodNotAllowed,
    LengthRequired,
    PayloadTooLarge,
    UnsupportedMediaType,
    RequestHeaderFieldsTooLarge,
    InternalServerError,
}

impl StatusCode {
    fn code(self) -> u16 {
        match self {
            Self::Ok => 200,
            Self::Accepted => 202,
            Self::NoContent => 204,
            Self::BadRequest => 400,
            Self::NotFound => 404,
            Self::MethodNotAllowed => 405,
            Self::LengthRequired => 411,
            Self::PayloadTooLarge => 413,
            Self::UnsupportedMediaType => 415,
            Self::RequestHeaderFieldsTooLarge => 431,
            Self::InternalServerError => 500,
        }
    }

    fn reason(self) -> &'static str {
        match self {
            Self::Ok => "OK",
            Self::Accepted => "Accepted",
            Self::NoContent => "No Content",
            Self::BadRequest => "Bad Request",
            Self::NotFound => "Not Found",
            Self::MethodNotAllowed => "Method Not Allowed",
            Self::LengthRequired => "Length Required",
            Self::PayloadTooLarge => "Payload Too Large",
            Self::UnsupportedMediaType => "Unsupported Media Type",
            Self::RequestHeaderFieldsTooLarge => "Request Header Fields Too Large",
            Self::InternalServerError => "Internal Server Error",
        }
    }
}

pub(super) fn write_text_response(
    stream: &mut impl std::io::Write,
    status: StatusCode,
    message: &str,
) -> Result<()> {
    write_binary_response(
        stream,
        status,
        Some("text/plain; charset=utf-8"),
        message.as_bytes(),
    )
}

pub(super) fn write_empty_response(
    stream: &mut impl std::io::Write,
    status: StatusCode,
) -> Result<()> {
    write_binary_response(stream, status, None, &[])
}

fn write_binary_response(
    stream: &mut impl std::io::Write,
    status: StatusCode,
    content_type: Option<&str>,
    body: &[u8],
) -> Result<()> {
    write!(stream, "HTTP/1.1 {} {}\r\n", status.code(), status.reason())?;
    if let Some(content_type) = content_type {
        write!(stream, "Content-Type: {content_type}\r\n")?;
    }
    write!(
        stream,
        "Content-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    )?;
    stream.write_all(body)?;
    stream.flush()?;
    Ok(())
}
