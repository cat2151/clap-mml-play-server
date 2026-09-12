# ADR 0012: 実測ベースライン（退行検知用）

- 状態: 記録
- 関連: [0008](0008-spare-instance-pool.md) / [0009](0009-unsafe-thread-handoff.md) /
  [0013](0013-serial-instantiation.md)

Windows / release ビルド / 48kHz / buffer 512。数値は判断に効く物だけ残している。

## インスタンス生成のコスト（`CMRT_LIVE_INSTANCE_COUNT=16`）

- Dexed は 1 個 1〜22ms、Surge XT（warm）は 1 個 201〜362ms。**予備プールの補充コストがこれ**
- Surge を 12 並列で作ると 1 個あたり約 350ms（cold なら 3013ms）、単独なら約 205ms。
  **並列が効いているのではなく取り合っている**
- **ベースライン比較は必ず 2 回目以降で取ること。** cold は 16 個で 3.2 秒まで伸びる

## `examples/parallel_instance_creation.rs` での測り方

example が自前で entry をロードして N 個作る**別の測り方**。
`SURGE_DATA_HOME` の最小化（[0010](0010-surge-data-home-and-plugin-identity.md)）を通らないので、
**example どうしの比較には使えるが、上の Surge の数字と直接は比べないこと。**

- Vaporizer2 3.5.0 は 1 個 97〜107ms（直列 8 個 845ms。並列にしても直列化されるので同じ。
  [0013](0013-serial-instantiation.md)）。**Surge XT（約 530ms/個）の 1/5 の時間で作れる**
- cold（プロセス初回）は Vaporizer2 で 136ms、Surge で 1649ms

### メモリ: Vaporizer2 だけが高い

直列に作って保持したまま測った working set は**約 89MB / instance** で線形
（Surge XT・Dexed は約 12MB。7 倍）。drop も効く（8 個の解放 146ms）。

- 16 スロット x 3 種別の予備前払いなら Vaporizer2 の予備 8 個だけで約 710MB。
  最悪ケース（16 スロット全部 Vaporizer2 + 予備 8）は約 2.1GB
- **判断: 既定値は据え置き。** 減らす口は `CMRT_SPARE_INSTANCES`（`1` で前払いをやめ、
  `0` で予備プールごと止まる）。既定を下げると Surge / Dexed 環境まで遅くなる
- 3 種別構成（8 スロット）の前払いは Vaporizer2 8 個で 0.9〜1.0 秒。増えるのは時間より
  **メモリ**（204MB → 947MB）。既定プラグインの予備は 0 のまま（[0008](0008-spare-instance-pool.md)）

### `.vvp` ヘッダ走査（460 件の先頭 4096 バイト）

warm 40ms / cold 近似 51ms。真の cold（OS のファイルキャッシュを空にした状態）は
RAMMap か再起動が要るので作れない。

### インスタンスごとのプリセットスキャンは、このマシンでは発現していない

Vaporizer2 のコンストラクタはプリセット走査を非同期で投げる（[0014](0014-vvp-as-clap-state.md)）が、
`%APPDATA%\Vaporizer2\VASTvaporizerSettings.xml` の `PresetRootFolder` が 0 件のディレクトリを
指していたので、上の 100ms/個に走査は入っていない（直列 8 個の per-instance ms が平らなのが証拠）。
**ユーザーがプラグイン側の preset root を大きなフォルダにしていれば伸びうる。**

## 起動（16 instance、`phase=listen` まで）

Dexed 59〜137ms / Surge XT 625〜756ms（warm 530ms と整合）。
`phase=surge_data_home` は Dexed で `skipped`、Surge で `ms=12 rebuilt=false`。

## レンダリング

- 無い cartridge / 無い program は 500 + 具体的なメッセージ。**黙って既定 program へ
  フォールバックしない**
- Dexed の実物 cartridge は 33 files × 32 program = 1,056 program
- **同じ program を選び直しても 2e-5 程度の差が残る**（LFO 位相などプラグイン内部の状態）。
  音の同一性の閾値は `SAME_SOUND_TOLERANCE = 0.001`
- **Surge は同一プロセスで同じ MML を 2 回レンダリングしてもサンプルが一致しない**
  （初期パッチのランダム位相などプラグイン側の性質。host の変更とは無関係）。
  **「出力が 1 bit も変わらないこと」を回帰テストの条件にしてはいけない**

## 混在の実機確認

render server（`POST /render`）へ Dexed の cartridge・Surge の `.fxp`・音色無指定の MML を送り、
WAV の sha256 が全部違う（**「操作は成功したが前の音のまま」になっていない**）ことと、
起動ログに 2 プラグインぶんの descriptor が出ることを確認した。判別の仕組みは
[0007](0007-patch-string-decides-the-plugin.md)。

## 番人テスト

| テスト | 落ちたら |
|---|---|
| `core-lib/src/render/tests/surge.rs::surge_still_advertises_a_stereo_main_output_and_the_clap_note_dialect` | Surge が MIDI dialect 経路へ落ちている（出音が変わりうる） |
| `core-lib/src/render/tests/cartridge.rs::dexed_mono_mode_stays_poly_for_every_program` | TUI 側 `AssumePoly` の前提が崩れている |
| `core-lib/src/render/tests/cartridge.rs::dexed_single_voice_sysex_sounds_like_the_program_change_reference` | packed voice の展開が壊れた（「鳴ってはいるが別の音」になる） |
| `core-lib/src/render/tests/dexed.rs::dexed_survives_parallel_creation_handoff_and_playback_of_sixteen_instances` | unsafe thread handoff が Dexed で壊れた |
| `core-lib/src/render/tests/vaporizer2.rs::vaporizer2_loads_every_patch_version_as_state_and_makes_sound` | `.vvp` の state 化が壊れた（[0014](0014-vvp-as-clap-state.md)） |
| `core-lib/src/render/tests/vaporizer2.rs::retagging_a_v2_00000_patch_changes_what_it_sounds_like` | V2.00000 の版読み替えが効いていない（**名前では検出できない**） |
| `core-lib/src/patch_list/tests/installed.rs::installed_cartridges_all_parse` | 実物 cartridge のパースが壊れた |
| `core-lib/src/patch_list/tests/installed.rs::installed_vaporizer2_presets_are_all_listed` | `.vvp` の列挙が壊れた（実物 460 件） |
| `core-lib/src/render/serial_instantiation/tests.rs` の 4 本 | 生成の直列化が壊れた（意味は [0013](0013-serial-instantiation.md) の表） |
| `player::instances::tests::pool_policy::a_prepaid_spares_are_all_ordered_up_front` | 予備の前払いがアイドル中に 1 個で止まる |
| `player::instances::tests::swapping::swapping_plugins_under_a_running_render_loop_survives_many_cycles` | 演奏中の跨ぎ差し替えが壊れた（2 プラグイン） |
| `player::instances::tests::swapping::swapping_across_three_plugins_under_a_running_render_loop_survives_many_cycles` | 同（3 プラグイン。60 回） |
| `player::instances::tests::cost::prepaying_spares_for_three_plugins_costs_this_much_time_and_memory` | 3 種別の前払いコストが測れなくなった（数字を print するテスト） |

テスト名が実装の改名で古びていないかは TUI 側 `scripts/check_adr_test_names.py` が機械で見る。

## 計測手順（再現用）

```bash
CMRT_LIVE_INSTANCE_COUNT=16 timeout 40 ./target/release/clap-mml-realtime-play-server.exe 2> err.log
grep -o "phase=[a-z_]* ms=[0-9]*" err.log
grep -o "phase=instance index=[0-9]* ms=[0-9]*" err.log
```

- **`timeout` で必ず落とすこと。** 孤児サーバーは SHM を握って次回起動を壊す。
  終わったら `tasklist | grep -i clap-mml` で残っていないことを確認する
- render server はプラグインを替えて試すのに実 config.toml を書き換えなくてよい。
  `clap-mml-render-server --config <PATH>` を取る（`ServerConfig::load_from_path()`）。
  TUI 側から使うときの罠は clap-mml-render-tui `docs/adr/0011-verification-and-baselines.md`

予備プールの検証:

```bash
CMRT_TEST_SURGE_CLAP='C:\Program Files\Common Files\CLAP\Surge Synth Team\Surge XT.clap' \
CMRT_TEST_DEXED_CLAP='C:\Program Files\Common Files\CLAP\Dexed.clap' \
CMRT_TEST_DEXED_CARTRIDGES='C:\Users\<user>\AppData\Roaming\DigitalSuburban\Dexed\Cartridges' \
CMRT_TEST_VAPORIZER2_CLAP='C:\Program Files\Common Files\CLAP\VASTvaporizer2.clap' \
cargo test -p clap-mml-realtime-play-server --release -- --include-ignored --test-threads=1 --nocapture instances
```

- Vaporizer2 の `.vvp` の置き場は環境変数ではなく、本番と同じ経路で config.toml の
  `[plugins.Vaporizer2] patches_dirs` から読む（無ければテストが落ちる）
- **`--test-threads=1` は必須。** Vaporizer2 のテストが 2 本同時に走ると
  [0013](0013-serial-instantiation.md) が守っていない形（別テスト由来の同時生成）になり、
  プロセスごと落ちる
- `cmrt-core` の実プラグインテストも同じ env で回す: `cargo test -p cmrt-core -- --ignored --test-threads=1`
- 並列生成に耐えるかの A/B（[0013](0013-serial-instantiation.md)）:
  `cargo run --release --example parallel_instance_creation -- "<CLAP のパス>" 8`
  （`CMRT_SERIAL_INSTANTIATION=off` を付けると直列化なしの側）
