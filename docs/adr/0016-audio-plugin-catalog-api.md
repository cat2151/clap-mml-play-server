# 0016: UI向けaudio plugin catalog APIをserver共有crateで提供する

## Status

Accepted

## Context

play serverはpluginのロード、patch形式、state変換を既に知っている。一方でclientが同じ
plugin判定を複製すると、対応plugin追加時にclient変更が必須となり、routing規則もずれる。

## Decision

`cmrt-core::audio_plugin`をplugin具象とclientの境界にする。公開APIは`PluginKey`、
`PatchRef`、`AudioPluginInfo`、`AudioPatch`、`AudioPluginCatalog`および抽象metadata型とする。
adapter固有の拡張子、カテゴリ配置、legacy lookup prefix、voicing取得方法はこのmodule内に置く。
用途別patch roleの組み込み既定は`cmrt-server-config`が返す。

serverの`PluginKind`も`PluginKey`を持ち、patch form判定は`audio_plugin`と同じ関数を使う。
同じpatch formを扱うpluginが複数あるとき、順序による選択はせず曖昧エラーを返す。
音色無指定時のdefault indexだけは従来の意味を維持する。

`PluginKey`はplugin IDがあればID、無ければ正規化したplugin pathから作る。これは内部参照と
再生成可能cacheに限定し、MMLやHTTP/IPC wire formatへは現時点で持ち込まない。

## Consequences

clientはplugin IDやpreset形式で分岐せず、server共有crateのcatalog結果を消費できる。
新しいadapter追加時の具象変更箇所はserver repositoryに閉じる。将来、同形式pluginを
wire越しに明示選択する必要が生じた場合は、別途protocol versionを設けて`PluginKey`を運ぶ。

