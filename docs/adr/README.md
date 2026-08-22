# ADR — 設計判断の記録

CLAP プラグイン（Surge XT / Dexed / Vaporizer2）の**実測仕様**と、サーバー側の**確定した設計判断**。
「なぜそうしなかったのか」と「再取得コストの高い実測値」を残している。

実装はすべて完了済み。

TUI 側（データ表現・config・カタログ）は `../clap-mml-render-tui/docs/adr/` にある。
依存の向きが TUI → play-server の一方向なので、ADR も repo ごとに閉じている。

| # | 決定 / 記録 |
|---|---|
| [0001](0001-measured-plugin-capabilities.md) | プラグインの実測仕様（descriptor と capability） |
| [0002](0002-capability-driven-ports-and-dialects.md) | audio port / note dialect は capability 駆動で決める |
| [0003](0003-dexed-program-change-guard.md) | Dexed の音色変更は single voice SysEx で送る |
| [0004](0004-syx-format-and-persistent-ids.md) | `.syx` の形式と、program の永続 ID |
| [0005](0005-dexed-mono-mode-is-poly.md) | Dexed の `MonoMode` は既定 POLY。生成時に設定しない |
| [0006](0006-no-generic-clap-preset-api.md) | CLAP 汎用 preset API を採らない |
| [0007](0007-patch-string-decides-the-plugin.md) | patch 文字列でプラグインを判別する（IPC / SHM は無改修） |
| [0008](0008-spare-instance-pool.md) | 予備インスタンスプール（論理スロットと物理インスタンスの分離） |
| [0009](0009-unsafe-thread-handoff.md) | unsafe thread handoff は測定で受け入れている（証明ではない） |
| [0010](0010-surge-data-home-and-plugin-identity.md) | `SURGE_DATA_HOME` 最適化は Surge 限定 / プラグイン同定の優先順位 |
| [0011](0011-clack-host-notes.md) | clack / host 実装の知識 |
| [0012](0012-measured-baselines.md) | 実測ベースライン（退行検知用） |
| [0013](0013-serial-instantiation.md) | 並列生成に耐えないプラグインだけ instance 生成を直列化する |
| [0014](0014-vvp-as-clap-state.md) | `.vvp` は CLAP state として流す（列挙も選択も host 側） |
