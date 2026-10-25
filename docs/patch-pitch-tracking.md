# 音色の鍵盤追従の測定

drum/percussion の分類には、マレット系やDexedの`PERCSYN`のように旋律を弾ける音色が混ざります。音色名では見分けられないため、実際に鳴らして「noteを変えると音高がついてくるか」（`tracks_keyboard`）を測ります。結果は`cat-music-patterns/patch-pitch-tracking.json`で共有し、各アプリがtrueの音色をleadへ移します。判定に音色名は使いません。

## 流れ

| 段 | 置き場 | 役割 |
|---|---|---|
| 1音色の計測 | このrepoの`probe-pitch-tracking` | patch文字列から`probe-voicing`と同じ経路でpluginを選んで鳴らし、判定を出す。pluginは問わない |
| 対象選択と一括計測 | clap-mml-render-tuiの`cmrt build-pitch-tracking` | cmrtのcatalogからdrumのpercussion行の音色を選び、1音色ごとに上を子プロセスで起動して表を書く |
| 共有 | `cat-music-patterns/patch-pitch-tracking.json` | 生成コマンドと形式は同repoのREADMEを参照 |
| 表示 | cat-plugin-player・cmrt・cat-smf-editor | 照合はclap-mml-render-tuiの`cmrt-patches`（`pitch_tracking`）を共有する |

対象選択をこのrepoに置かないのは、drum行の分類（`PatchRoleIndex`）がclap-mml-render-tuiにあり、そちらがこのrepoの`cmrt-core`に依存しているためです。このrepoから呼ぶと依存が循環します。1音色ごとに子プロセスにするのは、1つのpluginが落ちても全件の計測を止めないためです。

## 1音色の計測

```powershell
cargo run --release -p clap-mml-realtime-play-server -- probe-pitch-tracking --patch "patches_factory/Percussion/Drum One.fxp" --json
```

`--patch`はcatalogの表示名（`patches_dir`からの相対パス、Dexedは`cartridge.syx/番号 名前`）か絶対パスです。音声デバイスは開かず、config・catalog・音色stateを書き換えません。`--json`の出力は`{ "patch", "tracks_keyboard", "observations": [{ "note", "frequency_hz" }] }`で、推定できなかったnoteの`frequency_hz`は`null`です。

## 判定条件

48kHz・256frames、MIDI note 48・49・55・60、velocity 100・CC1=0で鳴らします。noteごとに新しいinstanceを作り（前のnoteの余韻を次の窓へ持ち込まないため）、4block空回ししてからnote-onし、約500msを録ります。

音高は[YINの累積平均正規化差分](https://pubmed.ncbi.nlm.nih.gov/12002874/)で推定し、明瞭な基音を持たないFM系の音のために、Hann窓FFTのスペクトルの山の移動も見ます。次のどちらかなら`tracks_keyboard=true`です。

- 4音すべてで周期性が0.85以上、かつnote 48の推定値からのずれがnoteの差と40cent以内
- note 48のスペクトルの山（窓内エネルギーの8%以上）のどれかについて、4音すべてに40cent以内で移動した山がある

解析範囲は35〜2000Hzです。閾値は校正していません。

## 制限

- 測定範囲外の音域・velocity、短すぎる音、強い変調、極端に小さい音量では検出できないことがあります。`false`は「この条件で確認できなかった」であり、旋律に使えないという断定ではありません
- 鍵盤に追従するclap等もtrueになり得ます。判定は用途の分類ではありません
- sforzandoなどサンプルを読む音色は、noteごとにinstanceを作り直すので1音色に数十秒かかることがあります
