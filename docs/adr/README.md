# ADR — 設計判断の記録

CLAP プラグイン（Surge XT / Dexed / Vaporizer2 と、effect の TONE3000 / Surge XT Effects）の**実測仕様**と、サーバー側の**確定した設計判断**。
「なぜそうしなかったのか」と「再取得コストの高い実測値」を残している。

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
| [0015](0015-sforzando-sfz-preset-load.md) | Sforzando の任意 SFZ は vendor state adapter でロードする |
| [0016](0016-audio-plugin-catalog-api.md) | UI 向け audio plugin catalog API は server 共有 crate で提供する |
| [0017](0017-fixed-surge-primary-plugin.md) | 共有 config の既定プラグインを Surge XT に固定する |
| [0018](0018-patch-load-must-not-spin-the-plugin.md) | 音色ロードの下準備で `process()` を空回ししてよいかはプラグインの契約で決める |
| [0019](0019-cache-player-slot-headroom.md) | cache-player のスロットは 4 本（クロックの先行を吸収する余裕） |
| [0020](0020-audio-effects-are-baked-into-the-offline-render.md) | audio effect は offline render で焼き込む。chain の位置は instrument 直後・gain 前 |
| [0021](0021-offline-render-serializes-instance-creation.md) | オフライン render は instance 生成を全 plugin 一律に直列化する |
