# その他
- cat2151のライブラリは、「revision固定を禁止。さらに、古い lock を放置せず最新 HEAD へ追従すること」
- issue-notes/は更新を禁止
- README.mdは更新禁止。README.ja.mdから生成されるので。

# 完了時
- 450行をoverした*.rsは、単一責任の原則に従いファイル分割
- cargoのclippyとfmtを使うこと
- デバッグビルド（ cargo build ）をすること（../clap-mml-render-tui/ にてcargo runで利用するので）
- プルリクエストは日本語で書くこと
