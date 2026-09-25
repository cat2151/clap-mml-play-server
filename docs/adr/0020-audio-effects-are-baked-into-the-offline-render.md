# ADR 0020: audio effect は offline render で焼き込み、live instance にも同じ位置で掛ける。chain の位置は instrument 直後・gain 前

- 状態: 採用
- 関連: [0006](0006-no-generic-clap-preset-api.md) / [0007](0007-patch-string-decides-the-plugin.md) /
  [0015](0015-sforzando-sfz-preset-load.md) / [0016](0016-audio-plugin-catalog-api.md) /
  clap-mml-render-tui `docs/adr/0022-effect-chain-in-init-json.md`

## 決定

instrument の後段に直列で挿す CLAP audio effect（TONE3000 / Surge XT Effects）は、
**offline render の pipeline で cache WAV に焼き込む**。live instance も同じ chain を持てる（下の「live instance の chain」）。

- chain を通す位置は **instrument の出力直後、preroll の trim と instance gain の前**
  （`core-lib/src/pipeline/effects.rs`）。live でも bank worker の render 直後（auto gain と mix の前）に置き、
  offline と live で音が変わらないようにする
- chain の指定は MML 先頭 JSON の `"effects after instrument"`（配列の順 = 信号の順、各要素はキー 1 つの
  オブジェクト）。catalog（`AudioEffectCatalog`）が `(json_key, value)` を返し、client は plugin 名で
  分岐しない（[0016](0016-audio-plugin-catalog-api.md) と同じ境界）
- **effect を持たない経路に chain 付きの MML が来たらエラー**にする（`RenderEffects::unsupported`）。
  黙って dry で返すと、出音が違うのに成功に見える
- chain 要素は「plugin を決めるキー 1 つ」＋任意の `bypass`（bool）キーを持てる
  （`EFFECT_STAGE_BYPASS_JSON_KEY`）。`bypass: true` の段は parse 時（`stage_from_element`）に
  chain から落ちる。`apply` はこの段の存在を知らない

## live instance の chain

DAW の演奏は「cell MML → offline render → cache WAV → cache-player」のままで、chain を JSON に書けば
再 render は自動で走る。live に chain を持たせるのは、cache の無い音を待たせずに鳴らすため
（TUI の EFFECT CHAIN の試聴。clap-mml-render-tui ADR 0022）。

- chain は **live instance ごと**に持ち、**音色の準備（`PreparePatch` / `PrepareStandbyPatch`）に同梱**する。
  SHM の command に chain の JSON（`"effects after instrument"` の値。空 = chain 無し）の欄がある。
  別の IPC にすると、音色と chain の差し替えの間に「新しい音色 + 古い chain」の block が鳴りうる
- 準備は bank worker のスレッドで走り、chain は先に作る。作れなければ音色も読まずに準備を失敗させる
  （前の chain は残る）。音色が今と同じで chain だけが違う要求では instrument を読み直さない
  （`bank/instance_chain.rs`）。**chain 無しの準備を同じ音色で送ると、それで chain が外れる**ので、
  client は「準備済みか」の比較に chain も入れること
- chain は bank の instance ごとの後処理（`bank/post_process.rs`）として render の直後に通す。
  後処理の失敗は instance の render 失敗と同じ扱いで、停止と render 失敗では chain も reset する
  （前の reverb の余韻が次の演奏へ漏れないため）
- effect の catalog の走査と DLL のロードは、最初の chain 付きの準備まで遅らせる（`bank/live_chain.rs`）。
  chain 無しの利用者の起動を待たせないため
- latency 補償はしない。Surge XT Effects で 32 frame（0.7 ms）
- 指定した instance 群を N ms で fadeout できる（`FadeOutInstances`。応答は無い）。ramp は chain の後・auto gain の前に
  sample 単位の直線で掛け（`bank/fade.rs`）、generation は上げない（上げると出力リングの描画済み frame が段差で捨てられる）。
  timeline の張り直しでも fadeout は捨てない（client は「fadeout → 次の行」の順に送る）
- 0 に達した instance は voice と chain を reset したうえで、**出力を 0 に保つ**。Surge XT Effects の reverb は CLAP の reset でも
  deactivate / activate でも余韻が消えず、reset だけでは -43 dB まで落ちた後に -31 dB へ戻った。0 を解くのは新しい行の開始
  （張り直しか、その instance の音色の準備）の後にイベントが届いたとき。fade 中に届いた新しい行のイベントは、0 に達した後の
  block の頭へ預ける（fade 中に鳴らすと頭が絞られ、reset で消える）

部品に置いた制約（live から呼ぶための前提）:

| 制約 | 形 |
|---|---|
| `EffectRenderer::process` は RT で呼べる | interleaved `&mut [f32]` を in-place。確保・String・ログ無し。生成（`new`）と preset 適用（`load_*`）は別の関数 |
| `EffectChainSpec` は MML から独立した値型 | JSON からの parse は別関数（`effect_chain_spec_from_embedded_json`）。spec → chain の構築は `RenderEffects::build_chain` で offline と live が共有する |
| 位置は instrument 直後・gain 前 | 上記 |

## instrument の renderer を流用しなかった理由

`RealtimeRenderer` は note port を要求し、input port を 1 本しか渡さない。Surge XT Effects は
note port 0 本・input port 2 本（port 1 は sidechain）なので通らない。受け入れ条件を緩めると
instrument 側の「note port が要る」保証が消えるので、`EffectRenderer` を別に持ち、
note port を要求せず input port を広告本数ぶん全部渡す（port 0 に信号、残りは無音）。

## preset は file → state を host が組む

両 plugin とも preset-discovery / preset-load 拡張を持たない（`probe-capabilities` の実測）。
[0006](0006-no-generic-clap-preset-api.md) / [0015](0015-sforzando-sfz-preset-load.md) と同じく、
preset ファイルから plugin state を host が組んで `clap.state.load` へ渡す。

- Surge XT Effects: state は JUCE XML で、parameter は **GUI 順・0..1 正規化**。`.srgfx` は
  storage 順・実値なので、並びは Surge のソースから生成した表で引き直し、正規化の範囲は
  **plugin に全 parameter 0 / 1 の state を読ませて保存させ、自己申告から測る**
  （`core-lib/src/surge_fx_preset.rs`）。範囲は plugin にしか無い
- TONE3000: preset も state も `T3KB` + JUCE `ValueTree`。生成直後の state を template に
  `ChainSnapshot` を差し替え、`PARAMETERS` を上書きし、`activePresetId/Name` を書く
  （`core-lib/src/tone3000_preset.rs`）。名前だけ書いても plugin は preset を読まない

## latency と block 境界

chain は各段の `clap.latency` を合計し、末尾に無音を足して回してから先頭を捨てるので、
出力の位置は入力と揃う（`EffectChain::process_all`）。

- Surge XT Effects は端数 block を 1 回でも受けると latency 32 の mode へ切り替わるので、
  `buf_size` の倍数まで無音で埋めて回す
- 申告 32 に対して実際の遅れは 31。1 sample/段の残差は可聴外なので補正しない

## 踏んだ罠（守ること）

- Surge XT Effects は **`fxt=0` のまま activate すると Delay が載る**。「effect off」の instance は
  作れないので、chain に入れる段は必ず preset を載せてから activate する
- TONE3000 は state load 後に model を非同期で読み、readiness API が無い。正弦波を通して
  出力が -60 dBFS を超えるまで待つ（上限 20 s）のを preset 適用の一部にしている。
  instance は preset ごとに新規生成し init state から載せる

## 値段と残している論点

- render ごとに effect instance を新規生成する。Surge XT Effects は生成 72〜105 ms + preset 2 ms、
  TONE3000 は preset 適用込みで 114〜928 ms（model の大きさに比例）
- chain は instrument と同じサンプル数だけ回すので、リバーブの尻尾は WAV 末尾で切れる
  （Cathedral 2 で末尾 1 s が -46 dBFS）。render の延長は入れていない
- live の chain の準備はその bank の render を止める（Surge XT Effects 1 段で 80〜260 ms）。同じ instance へ
  続けて準備すると、そこで鳴っていた effect の余韻は切れる。先にその instance を fadeout しておけば段差にならない
- fadeout は render に掛かるので、device へ出るのは出力リングの分（約 50 ms）後。50 ms の fadeout で、送信から約 100 ms で 0 になる
- render-server backend も effect を持つ（`render-server/src/main.rs` が boot で `EffectPlugins` を
  1 つ持ち、worker 間で `Arc` 共有する）

## 壊れたら気づく場所

| テスト | 落ちたら |
|---|---|
| `pipeline::tests::effects::unsupported_route_rejects_a_chain_before_rendering` | chain 付き MML が黙って dry で通るようになった |
| `audio_effect::tests::unknown_key_is_an_error` / `unlisted_preset_is_an_error` / `ambiguous_preset_name_is_an_error` | JSON の形と catalog の照合が緩んだ |
| `effect::tests::surge_fx::surge_fx_chain_aligns_output_with_input`（ignored） | latency 補正か block 境界の扱いが崩れた |
| `effect::tests::surge_fx::surge_fx_every_factory_snapshot_matches_self_report`（ignored） | 並びか正規化の範囲が plugin と食い違った |
| `effect::tests::tone3000::tone3000_every_factory_preset_loads_by_name`（ignored） | template への書き込みが plugin に通らなくなった |
| `pipeline::tests::effects::cache_render_with_a_reverb_chain_differs_from_dry_and_both_are_audible`（ignored） | pipeline の適用位置か transport が壊れた |
| `bank::instance_chain::tests::the_bundled_chain_applies_to_its_instance` / `an_empty_chain_keeps_the_dry_path` | 準備に同梱した chain が live instance に掛からない、または chain 無しで出力が変わった |
| `bank::instance_chain::tests::changing_only_the_chain_does_not_reload_the_patch` | chain だけの差し替えで instrument を読み直すようになった |
| `bank::instance_chain::tests::a_chain_that_fails_to_build_fails_the_prepare_without_loading` | chain の生成失敗が黙って dry で鳴る |
| `bank::fade::tests::only_the_faded_instance_reaches_zero_in_the_fade_length` / `the_ramp_has_no_step_even_when_restarted` | fadeout が段差になった、または他の instance まで絞った |
| `bank::fade::tests::the_old_line_is_dropped_and_the_new_line_sounds_at_full_gain` / `events_during_the_fade_wait_until_it_ends` | fadeout した行の余韻が戻る、または次の行の頭が絞られる |
| `commands::tests::a_fade_out_survives_the_next_timeline_and_keeps_the_generation` | 次の行の張り直しで fadeout が捨てられる、または出力リングが段差で捨てられる |
