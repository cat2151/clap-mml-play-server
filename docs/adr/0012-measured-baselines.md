# ADR 0012: 実測ベースライン（退行検知用）

- 状態: 記録（2026-08-20 実測）
- 関連: [0008](0008-spare-instance-pool.md) / [0009](0009-unsafe-thread-handoff.md)

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

| テスト | 落ちたら |
|---|---|
| `core-lib/src/render/tests.rs::surge_still_advertises_a_stereo_main_output_and_the_clap_note_dialect` | Surge が MIDI dialect 経路へ落ちている（出音が変わりうる） |
| `core-lib/src/render/tests/cartridge.rs::dexed_mono_mode_stays_poly_for_every_program` | TUI 側 `AssumePoly` の前提が崩れている |
| `core-lib/src/render/tests/cartridge.rs::dexed_single_voice_sysex_sounds_like_the_program_change_reference` | packed voice の展開が壊れた（「鳴ってはいるが別の音」になる） |
| `core-lib/src/render/tests.rs::dexed_survives_parallel_creation_handoff_and_playback_of_sixteen_instances` | unsafe thread handoff が Dexed で壊れた |
| `core-lib/src/patch_list_tests.rs::installed_cartridges_all_parse` | 実物 cartridge のパースが壊れた |
| `player::instances::tests::a_prepaid_spares_are_all_ordered_up_front` | 予備の前払いがアイドル中に 1 個で止まる |
| `player::instances::tests::swapping_plugins_under_a_running_render_loop_survives_many_cycles` | 演奏中の跨ぎ差し替えが壊れた |

## 計測手順（再現用）

```bash
CMRT_LIVE_INSTANCE_COUNT=16 timeout 40 ./target/release/clap-mml-realtime-play-server.exe 2> err.log
grep -o "phase=[a-z_]* ms=[0-9]*" err.log
grep -o "phase=instance index=[0-9]* ms=[0-9]*" err.log
```

- **`timeout` で必ず落とすこと。** 孤児サーバーは SHM を握って次回起動を壊す。
  終わったら `tasklist | grep -i clap-mml` で残っていないことを確認する
- プラグインを替えるには `config.toml` の `active_plugin` を書き換えるしかない
  （`config_app_dir()` に env の差し替え口が無い）。
  **必ずバックアップを取り、終わったら差分ゼロまで戻すこと**

予備プールの検証:

```bash
CMRT_TEST_SURGE_CLAP='C:\Program Files\Common Files\CLAP\Surge Synth Team\Surge XT.clap' \
CMRT_TEST_DEXED_CLAP='C:\Program Files\Common Files\CLAP\Dexed.clap' \
CMRT_TEST_DEXED_CARTRIDGES='C:\Users\<user>\AppData\Roaming\DigitalSuburban\Dexed\Cartridges' \
cargo test -p clap-mml-realtime-play-server --release -- --include-ignored --test-threads=1 --nocapture instances
```
