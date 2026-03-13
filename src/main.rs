mod config;
mod host;
mod midi;
mod patch_list;
mod pipeline;
mod render;
mod tui;

use anyhow::Result;

fn main() -> Result<()> {
    // 引数なし → TUI モード
    // --help / -h → ヘルプ表示（config パスを含む）
    // --mml "cde" → CLI パイプラインモード（テスト用）
    let args: Vec<String> = std::env::args().collect();

    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("cmrt - CLAP MML Render TUI");
        println!();
        println!("使い方:");
        println!("  cmrt              TUI モードで起動");
        println!("  cmrt --mml <mml>  CLI モード（テスト用）");
        println!("  cmrt --help       このヘルプを表示");
        println!();
        match config::config_file_path() {
            Some(p) => println!("設定ファイル: {}", p.display()),
            None => println!("設定ファイル: (システムの設定ディレクトリが見つかりません)"),
        }
        return Ok(());
    }

    let cfg = config::Config::load()?;

    // CLAP プラグインエントリをロード（TUI/CLI 共通）
    let entry = host::load_entry(&cfg.plugin_path)?;

    if let Some(pos) = args.iter().position(|a| a == "--mml") {
        if let Some(mml) = args.get(pos + 1) {
            println!("CLI モード: MML = {}", mml);
            let patch = pipeline::mml_to_play(mml, &cfg, &entry)?;
            println!("patch: {}", patch);
            return Ok(());
        }
    }

    // TUI モード
    let mut app = tui::TuiApp::new(&cfg, &entry);
    app.run()?;

    Ok(())
}

