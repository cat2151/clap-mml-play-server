# ADR 0021: オフライン render は instance 生成を全 plugin 一律に直列化する

- 状態: 採用
- 関連: [0013](0013-serial-instantiation.md) / [0020](0020-audio-effects-are-baked-into-the-offline-render.md)

## 背景

render server の worker は request ごとに `render_to_memory` で instance を作って捨てる。
2 worker が同時に sforzando の音色で render すると、instance 生成（instantiate + SFZ ロード）が
重なった時点で `STATUS_ACCESS_VIOLATION` でプロセスごと落ちる。effect chain の有無・種類は無関係で、
Floe も同じ形で `Could not instantiate` を返す。逐次なら両方とも通る。

[0013](0013-serial-instantiation.md) の `RwLock` は `create_plugin()` + `init()` の区間だけを
plugin 別に守るので、音色ロードまで含めて重なると落ちる sforzando には届かない。

## 決定

`core-lib/src/render/offline.rs` にプロセス共通の `Mutex` を 1 本置き、`render` / `render_to_memory` の
`RealtimeRenderer::new`（instantiate から `start_processing()` まで）を **全 plugin 一律**に直列化する。
`render_next_chunk` は lock の外で並列のまま。

## 却下した案

| 案 | 却下理由 |
|---|---|
| [0013](0013-serial-instantiation.md) の表へ sforzando を足す | 守る区間が `create_plugin()` + `init()` だけで、音色ロードの重なりを防げない。Floe も落ちるので plugin を列挙する形では未知の plugin で同じ調査を繰り返す |
| render server 側で render 全体を直列化する | `process()` まで逐次になり、worker を複数持つ意味が無くなる |

## 結果

- 直列になるのは instance 生成だけ。Surge XT 等の生成は 100〜300ms で、render 本体は並列のまま
- realtime-play-server は `render_to_memory` を通らない（起動時の並列生成と予備プールは [0013](0013-serial-instantiation.md) のまま）
- 機械確認は TUI 側の `offline-render/src/render_server/tests/parallel_effect_chain.rs`
  （`CMRT_TEST_PARALLEL_RENDER_INIT_JSON` で音色を差し替えて 2 並列）
