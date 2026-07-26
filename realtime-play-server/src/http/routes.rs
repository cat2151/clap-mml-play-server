use super::*;
pub(super) fn handle_play_request(
    stream: &mut impl std::io::Write,
    player: &dyn PlayerHandle,
    request: HttpRequest,
) -> Result<()> {
    if request.header("content-length").is_none() {
        write_text_response(
            stream,
            StatusCode::LengthRequired,
            "Content-Length required",
        )?;
        return Ok(());
    }
    if !request
        .header("content-type")
        .is_some_and(content_type_is_midi)
    {
        write_text_response(
            stream,
            StatusCode::UnsupportedMediaType,
            "Content-Type must be audio/midi, audio/x-midi, or application/octet-stream",
        )?;
        return Ok(());
    }
    match player.play_smf(request.body) {
        Ok(()) => write_text_response(stream, StatusCode::Accepted, "accepted")?,
        Err(error) => write_internal_error(stream, error)?,
    }
    Ok(())
}

pub(super) fn handle_play_mml_request(
    stream: &mut impl std::io::Write,
    player: &dyn PlayerHandle,
    request: HttpRequest,
) -> Result<()> {
    if request.header("content-length").is_none() {
        write_text_response(
            stream,
            StatusCode::LengthRequired,
            "Content-Length required",
        )?;
        return Ok(());
    }
    if !request
        .header("content-type")
        .is_some_and(content_type_is_text)
    {
        write_text_response(
            stream,
            StatusCode::UnsupportedMediaType,
            "Content-Type must be text/plain",
        )?;
        return Ok(());
    }
    let Ok(mml) = String::from_utf8(request.body) else {
        write_text_response(
            stream,
            StatusCode::BadRequest,
            "request body must be valid UTF-8",
        )?;
        return Ok(());
    };
    match player.play_mml(mml) {
        Ok(()) => write_text_response(stream, StatusCode::Accepted, "accepted")?,
        Err(error) => write_internal_error(stream, error)?,
    }
    Ok(())
}

pub(super) fn handle_stop_request(
    stream: &mut impl std::io::Write,
    player: &dyn PlayerHandle,
) -> Result<()> {
    match player.stop() {
        Ok(()) => write_empty_response(stream, StatusCode::NoContent)?,
        Err(error) => write_internal_error(stream, error)?,
    }
    Ok(())
}

fn write_internal_error(stream: &mut impl std::io::Write, error: anyhow::Error) -> Result<()> {
    write_text_response(
        stream,
        StatusCode::InternalServerError,
        &format!("{error:#}"),
    )
}

pub(super) fn content_type_is_midi(value: &str) -> bool {
    value.split(';').next().is_some_and(|media_type| {
        matches!(
            media_type.trim().to_ascii_lowercase().as_str(),
            "audio/midi" | "audio/x-midi" | "application/octet-stream"
        )
    })
}

pub(super) fn content_type_is_text(value: &str) -> bool {
    value
        .split(';')
        .next()
        .is_some_and(|media_type| media_type.trim().eq_ignore_ascii_case("text/plain"))
}
