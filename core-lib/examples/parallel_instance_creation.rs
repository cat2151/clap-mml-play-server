//! CLAP 1 本が「インスタンスのスレッド並列生成」に耐えるかを確かめ、生成コストを測る。
//!
//! 起動時の [`cmrt_core::create_renderers_parallel`] と予備インスタンスプール
//! （ADR 0008）は、**1 つの entry を共有したまま複数スレッドで instance を作る**。
//! これに耐えないプラグインを載せると、起動の途中で落ちる。
//!
//! CLAP の規約上は instance の生成は main thread 限定なので、耐えること自体が
//! 賭け（ADR 0009）である。対応プラグインを増やすときは必ずここで確かめること。
//!
//! ```text
//! cargo run --release --example parallel_instance_creation -- "<CLAP のパス>" [threads]
//! ```
//!
//! 直列でも同じ本数を作って比べる。**直列は通るのに並列で落ちる**なら、そのプラグインは
//! 並列生成に耐えない（プロセスごと segfault するので、終了コードでしか判定できない）。
//!
//! # 直列化を入れたあとの読み方
//! `cmrt_core::plugin_requires_serial_instantiation` が真のプラグインは、ホスト側で
//! 生成を直列化するので**並列フェーズも通る**。落ちることを確かめ直したいときは
//! `CMRT_SERIAL_INSTANTIATION=off` を渡す（A/B 用の逃げ口）。
//!
//! # 何が測れるか
//! 1 個あたりの生成時間（1 個目は cold、以降が warm のベースライン）と、
//! 直列 N 個を保持したままの working set。ADR 0012 の数字はここから取る。

use std::process::ExitCode;
use std::time::Instant;

use cmrt_core::{
    load_entry, plugin_requires_serial_instantiation, select_descriptor, CoreConfig,
    RealtimeRenderer,
};

const SAMPLE_RATE: f64 = 48_000.0;
const BUFFER_SIZE: usize = 512;

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let Some(plugin_path) = args.next() else {
        eprintln!("usage: parallel_instance_creation <CLAP のパス> [threads]");
        return ExitCode::FAILURE;
    };
    let threads: usize = args.next().map_or(8, |value| value.parse().unwrap());

    let load_started = Instant::now();
    let entry = load_entry(&plugin_path).unwrap();
    let load_entry_ms = load_started.elapsed().as_millis();
    let descriptor = select_descriptor(&entry, None).unwrap();
    println!("plugin: {}", descriptor.log_fields());
    println!(
        "load_entry_ms={load_entry_ms} serialized={} env={}",
        plugin_requires_serial_instantiation(&descriptor.id),
        std::env::var("CMRT_SERIAL_INSTANTIATION").unwrap_or_else(|_| "(未設定)".to_string()),
    );

    // 直列が通ることを先に確かめる。ここで落ちるなら並列とは別の問題。
    // **作ったものを持ったまま**測る。1 個ずつ捨てると、実運用（16 個が同時に生きている）と
    // メモリの出方が変わる。
    let serial_started = Instant::now();
    let mut kept = Vec::with_capacity(threads);
    let mut serial_ms = Vec::with_capacity(threads);
    for index in 0..threads {
        let started = Instant::now();
        kept.push(RealtimeRenderer::new(&config(), &entry).unwrap());
        serial_ms.push(started.elapsed().as_millis());
        println!("serial {index}: ok ({} ms)", serial_ms[index]);
    }
    println!(
        "serial ok ({threads} instances) total_ms={} per_instance_ms={serial_ms:?} working_set={}",
        serial_started.elapsed().as_millis(),
        working_set(),
    );
    let drop_started = Instant::now();
    drop(kept);
    println!("serial dropped ms={}", drop_started.elapsed().as_millis());

    // create_renderers_parallel と同じ形: entry は 1 つ、instance だけ並列に作る。
    let parallel_started = Instant::now();
    std::thread::scope(|scope| {
        for index in 0..threads {
            let entry = &entry;
            scope.spawn(move || {
                let started = Instant::now();
                let renderer = RealtimeRenderer::new(&config(), entry).unwrap();
                println!(
                    "parallel {index}: ok ({} ms, {:p})",
                    started.elapsed().as_millis(),
                    &renderer
                );
            });
        }
    });
    println!(
        "parallel ok ({threads} instances) total_ms={}",
        parallel_started.elapsed().as_millis()
    );

    ExitCode::SUCCESS
}

fn config() -> CoreConfig {
    CoreConfig {
        sample_rate: SAMPLE_RATE,
        buffer_size: BUFFER_SIZE,
        ..Default::default()
    }
}

/// 自プロセスの working set。インスタンスを保持したままのメモリ実コストを見る。
#[cfg(windows)]
fn working_set() -> String {
    let output = std::process::Command::new("tasklist")
        .args([
            "/FI",
            &format!("PID eq {}", std::process::id()),
            "/FO",
            "CSV",
            "/NH",
        ])
        .output();
    match output {
        // CSV の最終列がメモリ使用量。値そのものが桁区切りのカンマを含むので、
        // 区切りは `,` ではなく `","` で見る。
        Ok(output) => String::from_utf8_lossy(&output.stdout)
            .trim()
            .rsplit("\",\"")
            .next()
            .unwrap_or("?")
            .trim_matches('"')
            .to_string(),
        Err(error) => format!("(取得できず: {error})"),
    }
}

#[cfg(not(windows))]
fn working_set() -> String {
    "(Windows 以外では測っていない)".to_string()
}
