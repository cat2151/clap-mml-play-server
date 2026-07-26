use super::*;
use serde::Deserialize;

#[derive(Deserialize)]
struct LiveBufferRequestBody {
    multiplier: u8,
}

#[derive(Deserialize)]
struct MidiRequestBody {
    messages: Vec<[u8; 3]>,
    /// 各メッセージの発音位置（現在の live 位置からのフレーム数）。空なら全て 0。
    #[serde(default)]
    offsets: Vec<u32>,
    patch: Option<String>,
}

#[derive(Deserialize)]
struct LivePatchRequestBody {
    patch: Option<String>,
}

pub(super) fn handle_live_buffer_request(
    stream: &mut impl std::io::Write,
    player: &dyn PlayerHandle,
    request: HttpRequest,
) -> Result<()> {
    if !require_json(stream, &request)? {
        return Ok(());
    }
    let body: LiveBufferRequestBody = match parse_json(stream, &request, "live buffer")? {
        Some(body) => body,
        None => return Ok(()),
    };
    if !matches!(body.multiplier, 1 | 2 | 4 | 8) {
        write_text_response(
            stream,
            StatusCode::BadRequest,
            "buffer multiplier must be 1, 2, 4, or 8",
        )?;
        return Ok(());
    }
    match player.set_live_buffer_multiplier(body.multiplier) {
        Ok(()) => write_text_response(stream, StatusCode::Accepted, "accepted")?,
        Err(error) => write_internal_error(stream, error)?,
    }
    Ok(())
}

pub(super) fn handle_live_patch_request(
    stream: &mut impl std::io::Write,
    player: &dyn PlayerHandle,
    request: HttpRequest,
) -> Result<()> {
    let Some(body) = parse_live_patch(stream, &request)? else {
        return Ok(());
    };
    match player.prepare_live_patch(body.patch) {
        Ok(()) => write_empty_response(stream, StatusCode::NoContent)?,
        Err(error) => write_internal_error(stream, error)?,
    }
    Ok(())
}

pub(super) fn handle_live_patch_probe_request(
    stream: &mut impl std::io::Write,
    player: &dyn PlayerHandle,
    request: HttpRequest,
) -> Result<()> {
    let Some(body) = parse_live_patch(stream, &request)? else {
        return Ok(());
    };
    match player.prepare_live_patch_with_voicing(body.patch) {
        Ok(report) => write_json_response(stream, StatusCode::Ok, &report)?,
        Err(error) => write_internal_error(stream, error)?,
    }
    Ok(())
}

pub(super) fn handle_midi_request(
    stream: &mut impl std::io::Write,
    player: &dyn PlayerHandle,
    request: HttpRequest,
) -> Result<()> {
    if !require_json(stream, &request)? {
        return Ok(());
    }
    let body: MidiRequestBody = match parse_json(stream, &request, "MIDI")? {
        Some(body) => body,
        None => return Ok(()),
    };
    if body.messages.is_empty() {
        write_text_response(stream, StatusCode::BadRequest, "messages must not be empty")?;
        return Ok(());
    }
    if let Some(message) = body.messages.iter().find(|message| {
        !(0x80..=0xef).contains(&message[0]) || message[1] > 0x7f || message[2] > 0x7f
    }) {
        write_text_response(
            stream,
            StatusCode::BadRequest,
            &format!(
                "invalid MIDI channel voice message: [{}, {}, {}]",
                message[0], message[1], message[2]
            ),
        )?;
        return Ok(());
    }
    if !body.offsets.is_empty() && body.offsets.len() != body.messages.len() {
        write_text_response(
            stream,
            StatusCode::BadRequest,
            "offsets must be empty or the same length as messages",
        )?;
        return Ok(());
    }
    match player.send_midi(body.messages, body.offsets, body.patch) {
        Ok(()) => write_text_response(stream, StatusCode::Accepted, "accepted")?,
        Err(error) => write_internal_error(stream, error)?,
    }
    Ok(())
}

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

fn parse_live_patch(
    stream: &mut impl std::io::Write,
    request: &HttpRequest,
) -> Result<Option<LivePatchRequestBody>> {
    if !require_json(stream, request)? {
        return Ok(None);
    }
    parse_json(stream, request, "live patch")
}

fn require_json(stream: &mut impl std::io::Write, request: &HttpRequest) -> Result<bool> {
    if request.header("content-length").is_none() {
        write_text_response(
            stream,
            StatusCode::LengthRequired,
            "Content-Length required",
        )?;
        return Ok(false);
    }
    if !request
        .header("content-type")
        .is_some_and(content_type_is_json)
    {
        write_text_response(
            stream,
            StatusCode::UnsupportedMediaType,
            "Content-Type must be application/json",
        )?;
        return Ok(false);
    }
    Ok(true)
}

fn parse_json<T: serde::de::DeserializeOwned>(
    stream: &mut impl std::io::Write,
    request: &HttpRequest,
    label: &str,
) -> Result<Option<T>> {
    match serde_json::from_slice(&request.body) {
        Ok(body) => Ok(Some(body)),
        Err(error) => {
            write_text_response(
                stream,
                StatusCode::BadRequest,
                &format!("invalid {label} request JSON: {error}"),
            )?;
            Ok(None)
        }
    }
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

pub(super) fn content_type_is_json(value: &str) -> bool {
    value
        .split(';')
        .next()
        .is_some_and(|media_type| media_type.trim().eq_ignore_ascii_case("application/json"))
}
