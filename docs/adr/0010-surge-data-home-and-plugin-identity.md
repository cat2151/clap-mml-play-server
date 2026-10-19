# ADR 0010: `SURGE_DATA_HOME` 最適化は Surge 限定 / プラグイン同定の優先順位

- 状態: 採用
- 関連: [0007](0007-patch-string-decides-the-plugin.md)

## `SURGE_DATA_HOME` 最適化

### 起動から最初の発音までの最小化方針

起動から最初の発音までの時間を最小化する。同じ曲・音色・FX・音量で演奏できることを合格条件にし、ブラウザ専用の一覧構築や演奏に不要な資源探索を発音の前提にしない。環境変数で最小構成のdirectoryを指定できる場合は、その方法を先に実測する。

省略できる一覧と、発音に必要な初期patch・state内の音声資源を区別する。`Templates`を空にすると、その後FXPをロードしても出音が変わることがあるため、速度だけで採用を決めない。state復元、出音、失敗時の復帰を確認し、成功・失敗のどちらも本ADRへフィードバックする。

### 現行のpatch一覧削減

`apply_minimal_surge_data_home()` は Surge の起動を **8.8 秒 → 0.9 秒**にしている。

**`std::env::set_var` を使うので、スレッドを spawn する前に呼ぶ必要がある。**
適用の判定は「**プロセス内に Surge が 1 つでもあれば適用する**」（`apply_surge_data_home_for(kinds)`）。

### cat-smf-editorからの検証フィードバック

同じ`SURGE_DATA_HOME`の手法をFX preset・wavetablesにも適用し、次の結果を得た。Windows x64の同じSurge XTバイナリを使い、新規プロセスで5音源を直列生成した各3回の中央値である。serverの16音源起動時間とは区別する。

| envで指定したdata homeの構成 | `create_plugin`合計 | 判定 |
| --- | ---: | --- |
| patchだけ最小化、他の資源は保持 | 909ms | 比較元 |
| さらにFX presetを空にする | 498ms | 生成短縮を確認 |
| さらにfactory／3rd-party wavetablesを空にする | 530ms | 生成短縮を確認 |
| FX presetと両wavetablesを空にする | 129ms | 約86%短縮。Templatesは保持 |

出音検証は、実利用cacheの62音色／FX、元FXPの直接ロード48件、既定音色の計111ケースを各構成2回ずつ行った。`lastLoadedPath`だけは実際の参照先へcanonicalizeして比較し、音色パラメータ・wavetable payloadを含む復元後stateは全111ケースで一致した。元構成を独立したプロセスでも再実行し、そこで波形が安定していた42ケースは各省略構成の2回とも全sample一致した。Init Wavetable・Init FM2・Init Modernも別途確認した。

ランダム位相等で元構成から変動する音色を、bit一致条件の不合格にはしない（[0012](0012-measured-baselines.md)）。一方、state一致だけで全音色の聴感まで保証したとも扱わない。確認したFXP／stateではwavetableがpayloadに含まれていた。未知の外部wavetable参照、plugin更新、全patch、聴感はこの検証の保証範囲外である。

editorでは演奏用data homeをこの構成へ拡張し、元のSurgeインストール先と旧cacheを保全する。同じ新しいexe・既定5音源の実曲で通常起動を各3回測り、プロセス起動→非無音出力blockの中央値は1106msから280msへ約75%短縮した。browser・roots探索・state生成を行う子playerへは元の環境を渡し、一覧や資源を削らない。

ユーザーから、最適化後のeditorで普段使う曲の聴感確認を完了し、効果が実利用で発揮され、UXが劇的に改善されたとの報告を得た。これは上記機械検証に加わるeditorの実利用評価であり、起動から最初の発音までを最小化する方針の有効性を裏付ける。普段使う曲の聴感確認は完了済みとし、server本体への適用・検証状況とは区別する。

再現条件・数値・計測scriptは[editorの測定記録](../../../cat-smf-editor/docs/measurements/surge-resources-audio-20261019.md)に残す。

### serverでの採用: FX preset・wavetablesも空にする

`surge_data.rs`の最小構成で`fx_presets`・`wavetables`・`wavetables_3rdparty`を空directoryにする（`Templates`は従来どおり保持）。これらをjunctionで共有したままの旧構成は、作り直し判定で検出して組み直す。

serverはこれらの一覧を使わない。音色のwavetableはFXP／stateのpayloadから復元され、Surge XT Effectsのpresetは元の`fx_presets`を自前で読む（`server-config/src/effect_defaults.rs`）。

realtime-play-serverの16音源生成（`instances_total`）は934〜1084msから102〜123ms、プロセス起動→listen完了は約1.0〜1.2秒から約0.2秒になった。render-serverの`/render`で元のdata homeと比較し、元構成で決定的だった6音色は全sample一致、非決定的な音色・Surge XT Effects chainはRMS分布の重なりを確認した。聴感は未確認。

### 採らなかった代替案

**「`load_entry()` を先に呼んで descriptor を見てから `set_var`」**:
Surge の `clap_entry.init()` が data home を読む可能性があり、読んでいた場合に最適化が無効化される。
**実測せずに採用しないこと。**

## プラグイン同定の優先順位

**config の `plugin_id` があればそれだけで決める → 無いときだけ `plugin_path` のファイル名。**

### なぜファイル名判定を消さないか

production の `ServerConfig` は固定 Surge XT profile の `plugin_id` を解決済みで持つ。
ただし core の低レベル API と、`plugin_id` を省略した custom profile は `None` を渡せる。
その経路でも Surge の最適化を失わないため、ファイル名判定を fallback として残す。

## 罠

**`SURGE_XT_PLUGIN_ID` がこの repo 内 2 か所にある**
（`server-config/src/plugin_identity.rs` と `core-lib/src/surge_data.rs`）。片方だけ変えないこと。
