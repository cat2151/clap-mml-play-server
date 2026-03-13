//! サーバーモード: HTTP POSTでMMLを受け取りWAVデータを返す

use anyhow::Result;
use clack_host::prelude::PluginEntry;
use std::io::{Cursor, Read};

use crate::config::Config;
use crate::pipeline::mml_render;

pub const DEFAULT_PORT: u16 = 62151;

/// POSTボディのサイズ上限（バイト）。これを超えると 413 を返す。
const MAX_BODY_BYTES: u64 = 1024 * 1024; // 1 MiB

/// --server モードのメインループ
///
/// `port` でlistenし、POSTリクエストのbodyをMMLとして受け取り、
/// レンダリングしたWAVバイト列をレスポンスとして返す。
pub fn run_server(cfg: &Config, entry: &PluginEntry, port: u16) -> Result<()> {
    let addr = format!("127.0.0.1:{}", port);
    let server = tiny_http::Server::http(&addr)
        .map_err(|e| anyhow::anyhow!("HTTPサーバーの起動に失敗 ({}): {}", addr, e))?;
    println!("サーバーモード: http://{}/ でlistenしています", addr);
    println!("  POST /  - MMLをbodyで送信するとWAVデータを返します");
    println!("  Ctrl+C で終了");

    for mut request in server.incoming_requests() {
        let method = request.method().to_string();
        if method != "POST" {
            let response = tiny_http::Response::from_string(
                "POSTメソッドでMMLをbodyに含めて送信してください\n",
            )
            .with_status_code(405);
            let _ = request.respond(response);
            continue;
        }

        // bodyを読み取る（サイズ上限を設けてメモリ枯渇を防ぐ）
        let mut body = String::new();
        let reader = request.as_reader().take(MAX_BODY_BYTES + 1);
        let read_result = std::io::BufReader::new(reader).read_to_string(&mut body);
        if body.len() as u64 > MAX_BODY_BYTES {
            let response =
                tiny_http::Response::from_string("リクエストbodyが大きすぎます\n")
                    .with_status_code(413);
            let _ = request.respond(response);
            continue;
        }
        if let Err(e) = read_result {
            eprintln!("リクエストbodyの読み取りに失敗: {}", e);
            let response =
                tiny_http::Response::from_string("bodyの読み取りに失敗しました\n")
                    .with_status_code(400);
            let _ = request.respond(response);
            continue;
        }

        let mml = body.trim().to_string();
        if mml.is_empty() {
            let response =
                tiny_http::Response::from_string("MMLが空です\n").with_status_code(400);
            let _ = request.respond(response);
            continue;
        }

        let mml_preview: String = mml.chars().take(80).collect();
        println!("MML受信: {}", mml_preview.escape_default());

        match mml_render(&mml, cfg, entry) {
            Ok((samples, patch_display)) => {
                println!("レンダリング完了: patch={}", patch_display);

                // WAVをメモリ上に書き出す
                match samples_to_wav_bytes(&samples, cfg.sample_rate as u32) {
                    Ok(wav_bytes) => {
                        let response = tiny_http::Response::from_data(wav_bytes)
                            .with_header(
                                "Content-Type: audio/wav"
                                    .parse::<tiny_http::Header>()
                                    .expect("Content-Type ヘッダのパースに失敗"),
                            );
                        if let Err(e) = request.respond(response) {
                            eprintln!("レスポンス送信失敗: {}", e);
                        }
                    }
                    Err(e) => {
                        eprintln!("WAV変換失敗: {}", e);
                        let response =
                            tiny_http::Response::from_string(format!("WAV変換失敗: {}\n", e))
                                .with_status_code(500);
                        let _ = request.respond(response);
                    }
                }
            }
            Err(e) => {
                eprintln!("レンダリング失敗: {}", e);
                let response =
                    tiny_http::Response::from_string(format!("レンダリング失敗: {}\n", e))
                        .with_status_code(500);
                let _ = request.respond(response);
            }
        }
    }

    Ok(())
}

/// Vec<f32>（インターリーブステレオ）をメモリ上のWAVバイト列に変換する
fn samples_to_wav_bytes(samples: &[f32], sample_rate: u32) -> Result<Vec<u8>> {
    // 一時ファイルの代わりにメモリバッファに書き出す
    let mut buf: Vec<u8> = Vec::new();
    {
        use hound::{SampleFormat, WavSpec, WavWriter};
        let spec = WavSpec {
            channels: 2,
            sample_rate,
            bits_per_sample: 32,
            sample_format: SampleFormat::Float,
        };
        let cursor = Cursor::new(&mut buf);
        let mut writer = WavWriter::new(cursor, spec)
            .map_err(|e| anyhow::anyhow!("WAVWriter作成失敗: {}", e))?;
        for &s in samples {
            writer
                .write_sample(s)
                .map_err(|e| anyhow::anyhow!("WAVサンプル書き込み失敗: {}", e))?;
        }
        writer
            .finalize()
            .map_err(|e| anyhow::anyhow!("WAVファイナライズ失敗: {}", e))?;
    }
    Ok(buf)
}
