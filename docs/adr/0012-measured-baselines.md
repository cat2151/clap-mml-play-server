# ADR 0012: 実測ベースライン（退行検知用）

- 状態: 記録（2026-08-20 実測 / 2026-08-22 に Vaporizer2 を足して追記）
- 関連: [0008](0008-spare-instance-pool.md) / [0009](0009-unsafe-thread-handoff.md) /
  [0013](0013-serial-instantiation.md)

Windows / release ビルド / 48kHz / buffer 512。

## インスタンス生成のコスト（`CMRT_LIVE_INSTANCE_COUNT=16`）

| | `load_entry` | instance 1 個 | `instances_total`（16 個） |
|---|---|---|---|
| Dexed | 112ms | **1〜22ms** | 25ms |
| Surge XT（cold） | 104ms | 230ms（単独）〜3013ms（12 並列） | 3264ms |
| Surge XT（warm） | 36ms | **201〜362ms** | 530ms |

読み方:

- 並列数 12 のときの 1 個あたりが 3013ms（cold）/ 約 350ms（warm）まで膨らみ、
  残り 4 個を単独で作るときは 230ms / 約 205ms。**並列が効いているのではなく取り合っている**
- **予備プールの補充コストがこれ。** Surge で 200〜360ms、Dexed で数 ms
- **ベースライン比較は必ず 2 回目以降で取ること。** cold は 3.2 秒まで伸びる

## インスタンス生成のコスト（2026-08-22 / `examples/parallel_instance_creation.rs`）

上の表とは**別の測り方**（example が自前で entry をロードして N 個作る）。
`SURGE_DATA_HOME` の最小化（[0010](0010-surge-data-home-and-plugin-identity.md)）を通らないので、
**example どうしの比較には使えるが、上の表の Surge 行と直接は比べないこと。**

| | `load_entry` | 1 個目 | 2 個目以降 | 直列 8 個 | 並列 8 個 |
|---|---|---|---|---|---|
| **Vaporizer2 3.5.0** | 22ms | 136ms | **97〜107ms** | 845ms | 895ms（直列化されるので同じ。[0013](0013-serial-instantiation.md)） |
| Surge XT 1.3.4 | 45ms | 551ms | 524〜544ms | 4275ms | **1086ms** |
| Dexed 1.0.1 | 15ms | 10ms | 1ms | 21ms | 10ms |

- **Vaporizer2 は Surge XT の 1/5 の時間で作れる。** 直列化しても Surge より速い
- cold（プロセス初回）は Vaporizer2 で 136ms、Surge で 1649ms

### メモリ: **Vaporizer2 だけが高い**

直列に n 個作り、**保持したまま**測った working set:

| n | 1 | 2 | 4 | 8 | 16 |
|---|---|---|---|---|---|
| Vaporizer2 | 118MB | 208MB | 386MB | 741MB | **1452MB** |

きれいに線形で **約 89MB / instance**（Surge XT・Dexed は約 12MB。**7 倍**）。
drop も効いていて 8 個の解放は 146ms。

- 16 スロット x 3 種別の予備前払いなら **Vaporizer2 の予備 8 個だけで約 710MB**
- 最悪ケース（16 スロット全部 Vaporizer2 + 予備 8）は 24 個 = **約 2.1GB**
- **判断: 既定値は据え置き。** 減らす口は `CMRT_SPARE_INSTANCES`（`1` で前払いをやめ、
  `0` で予備プールごと止まる）。既定を下げると Surge / Dexed 環境まで遅くなる

### 3 種別構成の起動と前払い（8 スロット / release）

```text
three_plugin_prepay slots=8 spare_target=8 startup_ms=969 kinds=["Surge XT", "Dexed", "Vaporizer2"]
  filled_ms=[Some(0), Some(19), Some(1035)] spares=[0, 8, 8] physical=24
  working_set_slots_only=204,124 K working_set_prepaid=946,964 K
```

- **前払いの追加コストは Vaporizer2 8 個で 0.9〜1.0 秒。** 増えるのは時間より**メモリ**
  （204MB → 947MB）
- 既定プラグインの予備は 0 のまま（自給自足。[0008](0008-spare-instance-pool.md)）。
  物理は `8 + 8 + 8 = 24`（16 スロットなら `16 + 8 + 8 = 32`）

### `.vvp` ヘッダ走査（460 件の先頭 4096 バイト）

| | 時間 |
|---|---|
| warm | **40 ms** |
| cold 近似（**まだ誰も読んでいないページ**を同じ 460 ファイルから読む） | **51 ms** |

真の cold（OS のファイルキャッシュを空にした状態）は **RAMMap か再起動が要るので作れない**。
env var や通常権限で標準ライブラリからキャッシュを落とす手は無い。

### インスタンスごとのプリセットスキャンは、このマシンでは発現していない

Vaporizer2 のコンストラクタは `reloadPresetArray` を非同期で投げる（[0014](0014-vvp-as-clap-state.md)）が、
`%APPDATA%\Vaporizer2\VASTvaporizerSettings.xml` の `PresetRootFolder` は **0 件のディレクトリ**を
指していた（読んだだけ。**書き換えていない**）。上の 100ms/個に走査は入っていない。

- 走査は非同期スレッドなので生成時間に直接は乗らない。乗るなら直列 8 個の per-instance ms が
  右肩上がりになるはずだが、実測は 97〜107ms で**平ら**
- ユーザーがプラグイン側の preset root を大きなフォルダにしていれば伸びうる

## 起動（16 instance、`phase=listen` まで）

| 項目 | 値 |
|---|---|
| realtime-play-server | Dexed **59〜137ms** / Surge XT **625〜756ms** |
| `phase=surge_data_home` | Dexed `ms=0 result=skipped` / Surge `ms=12 result=ok rebuilt=false` |

Surge の 625〜756ms は上表の warm 530ms と整合する。

## レンダリング

| 項目 | 値 |
|---|---|
| `POST /render` `l8cdefgab`（Dexed 初期音色） | 200 / 720,044 byte、peak 4123・RMS 1511・非ゼロ率 0.48 |
| `POST /render` 3 program 比較 | `Dexed_01.syx/00 Say Again.` peak 4360・rms 1257 / `01 LAURIE` peak 13659・rms 2162 / `SynprezFM/SynprezFM_01.syx/07 Ryches` peak 6087・rms 779（**全部違う音で非無音**） |
| エラー時の挙動 | 無い cartridge / 無い program は 500 + 具体的なメッセージ。**黙って既定 program へフォールバックしない** |
| 実物 cartridge | 33 files × 32 program = **1,056 program** |
| 音の同一性の閾値 | `SAME_SOUND_TOLERANCE = 0.001`。**同じ program を選び直しても 2e-5 程度の差が残る**（LFO 位相などプラグイン内部の状態） |

**Surge は同一プロセスで同じ MML を 2 回レンダリングしてもサンプルが一致しない**
（初期パッチのランダム位相などプラグイン側の性質。host の変更とは無関係）。
**「出力が 1 bit も変わらないこと」を回帰テストの条件にしてはいけない。**

## 混在の実機確認（`active_plugin = "Dexed"`）

render server（`POST /render`、`Content-Type: text/plain; charset=utf-8`）:

| 送った MML | 結果 |
|---|---|
| `{"Surge XT patch":"Dexed_01.syx/00 Say Again."}t120o4l4cde` | 200 / peak 0.133 |
| `{"Surge XT patch":"patches_factory/Basses/Bass 1.fxp"}t120o4l4cde` | 200 / peak 0.446（**Surge のインスタンスへ回った**） |
| `{"Surge XT patch":"Dexed_01.syx/03 PHAROH    "}t120o4l4cde` | 200 / 上の 2 つと別の波形 |
| `t120o4l4cde`（音色無指定） | 200 / 既定プラグイン（Dexed）の初期音 |

4 本とも WAV の sha256 が全部違う。
**「操作は成功したが前の音のまま」になっていない**ことの確認。起動ログにも 2 プラグインぶんの
descriptor が出る:

```text
cmrt-render-server: plugin id=com.digital-suburban.dexed name=Dexed ...
cmrt-render-server: plugin id=org.surge-synth-team.surge-xt name=Surge XT ...
```

## 番人テスト

**2026-08-22 に置き場が動いた**（`render/tests.rs` と `instances/tests.rs` を分割した）。
下は現行のパス。

| テスト | 落ちたら |
|---|---|
| `core-lib/src/render/tests/surge.rs::surge_still_advertises_a_stereo_main_output_and_the_clap_note_dialect` | Surge が MIDI dialect 経路へ落ちている（出音が変わりうる） |
| `core-lib/src/render/tests/cartridge.rs::dexed_mono_mode_stays_poly_for_every_program` | TUI 側 `AssumePoly` の前提が崩れている |
| `core-lib/src/render/tests/cartridge.rs::dexed_single_voice_sysex_sounds_like_the_program_change_reference` | packed voice の展開が壊れた（「鳴ってはいるが別の音」になる） |
| `core-lib/src/render/tests/dexed.rs::dexed_survives_parallel_creation_handoff_and_playback_of_sixteen_instances` | unsafe thread handoff が Dexed で壊れた |
| `core-lib/src/render/tests/vaporizer2.rs::vaporizer2_loads_every_patch_version_as_state_and_makes_sound` | `.vvp` の state 化が壊れた（[0014](0014-vvp-as-clap-state.md)） |
| `core-lib/src/render/tests/vaporizer2.rs::retagging_a_v2_00000_patch_changes_what_it_sounds_like` | V2.00000 の版読み替えが効いていない（**名前では検出できない**） |
| `core-lib/src/patch_list/tests.rs::installed_cartridges_all_parse` | 実物 cartridge のパースが壊れた |
| `core-lib/src/patch_list/tests.rs::installed_vaporizer2_presets_are_all_listed` | `.vvp` の列挙が壊れた（実物 460 件） |
| `core-lib/src/render/serial_instantiation/tests.rs` の 4 本 | 生成の直列化が壊れた（意味は [0013](0013-serial-instantiation.md) の表） |
| `player::instances::tests::pool_policy::a_prepaid_spares_are_all_ordered_up_front` | 予備の前払いがアイドル中に 1 個で止まる |
| `player::instances::tests::swapping::swapping_plugins_under_a_running_render_loop_survives_many_cycles` | 演奏中の跨ぎ差し替えが壊れた（2 プラグイン） |
| `player::instances::tests::swapping::swapping_across_three_plugins_under_a_running_render_loop_survives_many_cycles` | 同（3 プラグイン。60 回） |
| `player::instances::tests::cost::prepaying_spares_for_three_plugins_costs_this_much_time_and_memory` | 3 種別の前払いコストが測れなくなった（数字を print するテスト） |

**ADR に書いた番人テスト名が実装の改名で古びていないか**は
`python scripts/check_adr_test_names.py` が機械で見る（上の分割で実際に古びた）。

## 計測手順（再現用）

```bash
CMRT_LIVE_INSTANCE_COUNT=16 timeout 40 ./target/release/clap-mml-realtime-play-server.exe 2> err.log
grep -o "phase=[a-z_]* ms=[0-9]*" err.log
grep -o "phase=instance index=[0-9]* ms=[0-9]*" err.log
```

- **`timeout` で必ず落とすこと。** 孤児サーバーは SHM を握って次回起動を壊す。
  終わったら `tasklist | grep -i clap-mml` で残っていないことを確認する
- **render server はプラグインを替えて試すのに実 config.toml を書き換えなくてよい。**
  `--config <PATH>` を取る（2026-08-22 追加。`ServerConfig::load_from_path()`）:

  ```bash
  ./target/release/clap-mml-render-server.exe --config /path/to/try.toml
  ```

  TUI 側の `cmrt patch-roles --config` / `cmrt render-mml --config` と対になる。
  TUI から render server バックエンドを試すときは、その config の
  `offline_render_server_command` へ `clap-mml-render-server --config <同じ config>` と書く。
  **先頭を引用符にしないこと**（`cmd /C` が最初と最後の引用符を落として起動に失敗する）
- realtime-play-server の `active_plugin` を替えるほうは、まだ config.toml を書き換えるしかない
  （`config_app_dir()` に env の差し替え口が無い）。
  **必ずバックアップを取り、終わったら差分ゼロまで戻すこと**

予備プールの検証:

```bash
CMRT_TEST_SURGE_CLAP='C:\Program Files\Common Files\CLAP\Surge Synth Team\Surge XT.clap' \
CMRT_TEST_DEXED_CLAP='C:\Program Files\Common Files\CLAP\Dexed.clap' \
CMRT_TEST_DEXED_CARTRIDGES='C:\Users\<user>\AppData\Roaming\DigitalSuburban\Dexed\Cartridges' \
CMRT_TEST_VAPORIZER2_CLAP='C:\Program Files\Common Files\CLAP\VASTvaporizer2.clap' \
CMRT_TEST_VAPORIZER2_PRESETS='<.vvp の置き場>' \
cargo test -p clap-mml-realtime-play-server --release -- --include-ignored --test-threads=1 --nocapture instances
```

**`--test-threads=1` は必須。** Vaporizer2 のテストが 2 本同時に走ると
[0013](0013-serial-instantiation.md) が守っていない形（別スレッドではなく別テスト由来の
同時生成）になり、プロセスごと落ちる。

`cmrt-core` の実プラグインテストも同じ env 5 本で回す:

```bash
cargo test -p cmrt-core -- --ignored --test-threads=1   # 34 本
```

並列生成に耐えるかの A/B（[0013](0013-serial-instantiation.md)）:

```bash
cargo run --release --example parallel_instance_creation -- "<CLAP のパス>" 8
CMRT_SERIAL_INSTANTIATION=off cargo run --release --example parallel_instance_creation -- "<CLAP のパス>" 8
```
