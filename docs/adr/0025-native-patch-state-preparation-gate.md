# 0025: 一時 renderer から別 host へ音色 state を転送する検証ゲート

日付: 2026-10-10。状態: 採用。catでの実機転送ゲートと本番transactionの検証を通過。

## 背景

cat の承認済み全カタログ Random patch handoff は、Dexed・Floe・sforzando の
固有 loader を UAPMD へ移植せず、既存 cmrt loader の一時 instance で owned state を
準備する方式を第一案とする。既存 Surge API の契約（ADR 0024）は変更しない。

## 決定

`prepare_native_clap_patch_state` は厳密な CLAP ID、絶対 bundle path と解決済み patch
参照を受け取り、既存 renderer の生成・音色反映・保存・破棄を呼び出したスレッドで
完結させる。Dexed の cartridge program suffix は保持する。Floe は既存 loader の
pending 完了と callback 処理を使い、sforzando は初期 state と既存 program mapping を
使う。GUI、デバイス、server は起動せず、返すのは state bytes のみ。

cmrt の生成ロックは UAPMD と共有されない。呼び出し側は別 host の instance 生成を
排除しなければならない。一時 instance の破棄後に別 UAPMD instance へ state を
転送し、音声が出て、異なる patch が区別できることを cat の opt-in test で確認する。
成功した state.load だけでは成立とはしない。

## 制約と検証

Floe には同一 DLL の deinit 後の再 init に既知の制約がある。二つの host の entry
lifecycle は共通の所有権管理ではないため、この経路の成立を実機で先に確認する。
不成立なら API を本番へ接続せず、承認済み handoff の停止条件に従う。entry の恒久保持、
helper process、UAPMD 直接 loader の導入をこの検証のために追加しない。

実行コマンド: cat root で `cargo test native_preparation_cross_host_gate -- --ignored --test-threads=1 --nocapture`。
`RANDOM_GATE_PLUGIN` に対象の厳密 ID、`RANDOM_GATE_WORK_DIR` に実行 ledger の作業領域を渡す。
テストは既存カタログの二候補を shared reader で解決する。sforzandoはTableWarp2.sfzと
Airy Bells.ariaxを指定し、user configを更新しない。
実測結果は実行 ledger に記録する。全音源対応完了や人間の聴取確認はこの ADR だけでは主張しない。

2026-10-10のWindows実機で、Dexed program 0/1、Floe二つのCeltic Harp preset、
sforzando SFZ/ARIAについて準備後の別UAPMD instance復元・発音・音声差を確認した。
既存UAPMD instanceを保持したままworker準備→既存instance適用→保存→別instance復元も
通過した。Floeの準備は二回目の検証で約538ms/318msだった。外部sampleを二重に読む
方式であり、メモリ使用量の上限や他環境でのlifecycle安全性を保証する測定ではない。
追加測定で、Floeの二音色を順に準備・適用・別instance復元するテストプロセス全体の
ピークworking setは約935MiBだった（カタログ、current・temporary・restored instanceを
含み、準備instance単体の割り当て量ではない）。測定方法とログはhandoff作業領域に保存する。

カタログ向け一般APIはSurge、Six Sines、TyrellN6、Vaporizer2の検査付き純粋変換と、
この三音源のnative準備をまとめる。旧Surge専用APIは維持する。
