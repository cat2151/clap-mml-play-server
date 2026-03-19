use anyhow::Result;
use clap_mml_render_tui::{config, server, tui, updater};
use cmrt_core::{CoreConfig, load_entry, mml_to_play};

fn main() -> Result<()> {
    // 引数なし → TUI モード
    // --help / -h → ヘルプ表示（config パスを含む）
    // <mml> → CLI パイプラインモード（テスト用）
    // --server [port] → サーバーモード（デフォルト port 62151）
    let args: Vec<String> = std::env::args().collect();

    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("cmrt - CLAP MML Render TUI");
        println!();
        println!("使い方:");
        println!("  cmrt                    TUI モードで起動");
        println!("  cmrt <mml>              CLI モード（テスト用）");
        println!("  cmrt --server           サーバーモード（port {}）", server::DEFAULT_PORT);
        println!("  cmrt --server <port>    サーバーモード（指定port）");
        println!("  cmrt --shutdown         サーバーを停止（port {}）", server::DEFAULT_PORT);
        println!("  cmrt --shutdown <port>  サーバーを停止（指定port）");
        println!("  cmrt --help             このヘルプを表示");
        println!();
        println!("サーバーモードでは HTTP POST でMMLを受け取りWAVデータを返します。");
        println!("  例: curl -X POST http://127.0.0.1:{}/  --data 'cde'", server::DEFAULT_PORT);
        println!();
        match config::config_file_path() {
            Some(p) => println!("設定ファイル: {}", p.display()),
            None => println!("設定ファイル: (システムの設定ディレクトリが見つかりません)"),
        }
        return Ok(());
    }

    if let Some(pos) = args.iter().position(|a| a == "--shutdown") {
        let port = match args.get(pos + 1) {
            Some(s) => match s.parse::<u16>() {
                Ok(p) => p,
                Err(_) => {
                    eprintln!("ポート番号が不正です: {}", s);
                    std::process::exit(1);
                }
            },
            None => server::DEFAULT_PORT,
        };
        server::shutdown_server(port)?;
        println!("サーバー（port {}）にシャットダウン要求を送りました。", port);
        return Ok(());
    }

    let cfg = config::Config::load()?;

    // plugin_path が未設定の場合は設定ファイルを編集するよう案内する
    if cfg.plugin_path.is_empty() {
        let path_hint = match config::config_file_path() {
            Some(p) => p.display().to_string(),
            None => "(不明)".to_string(),
        };
        anyhow::bail!(
            "plugin_path が設定されていません。設定ファイルを編集して CLAP プラグインのパスを指定してください。\n設定ファイル: {}",
            path_hint
        );
    }

    // CLAP プラグインエントリをロード（TUI/CLI/サーバー 共通）
    let entry = load_entry(&cfg.plugin_path)?;

    // --mml は廃止済み。旧来の使い方をしたユーザーに新しい使い方を案内する
    if args.iter().any(|a| a == "--mml") {
        anyhow::bail!("`--mml` オプションは廃止されました。`cmrt <mml>` の形式で指定してください。\n例: cmrt cde");
    }

    if let Some(pos) = args.iter().position(|a| a == "--server") {
        // --server [port] の次の引数をポート番号として解釈する（省略時はデフォルト）
        let port = args
            .get(pos + 1)
            .and_then(|s| s.parse::<u16>().ok())
            .unwrap_or(server::DEFAULT_PORT);
        return server::run_server(&cfg, &entry, port);
    }

    // 引数が1つかつフラグでなければ CLI モード
    if args.len() == 2 && !args[1].starts_with('-') {
        let mml = &args[1];
        println!("CLI モード: MML = {}", mml);
        let core_cfg = CoreConfig::from(&cfg);
        let patch = mml_to_play(mml, &core_cfg, &entry)?;
        println!("patch: {}", patch);
        return Ok(());
    }

    // TUI モード
    let mut app = tui::TuiApp::new(&cfg, &entry);

    // バックグラウンドで自動アップデートチェックを開始する
    updater::spawn_update_check(std::sync::Arc::clone(&app.update_available));

    app.run()?;

    // アップデートが利用可能な場合、問答無用でアップデートを実行する
    if app.update_available.load(std::sync::atomic::Ordering::Relaxed) {
        // サーバーが起動していればシャットダウンしてからアップデートする（未起動の場合は無視）
        let _ = server::shutdown_server(server::DEFAULT_PORT);
        if let Err(e) = updater::run_foreground_update() {
            eprintln!("アップデートに失敗しました: {}", e);
        }
    }

    Ok(())
}
