#!/usr/bin/env python3
"""docs/adr/*.md に書いた番人テスト名が、実装の改名で古びていないか調べる。

ADR は「壊れたら気づく場所」「番人テスト」の節で、落ちたら何が壊れるかをテスト名で示す。
**このテスト名はコンパイラが見てくれない。** 実際、テストファイルを責務で分割したときに
ADR 側のパスが古くなり、次のセッションが存在しないテストを探すことになった。

やること: 上の 2 つの節に出てくる `バッククォート` の中からテスト関数名らしきものを拾い、
`fn <名前>` がこの repo（無ければ隣の repo）に在るかを見る。無ければ終了コード 1。

    python scripts/check_adr_test_names.py

隣の repo が見つからないときは、そこに在るはずの名前を「未確認」として数えるだけで落とさない
（この repo だけを clone した人が落ちないようにするため）。
"""

from __future__ import annotations

import argparse
import os
import re
import sys

# この 2 つの節の中だけを見る。ADR 本文には config のキー名や構造体のフィールド名も
# バッククォートで書いてあるので、節を絞らないと誤検出だらけになる。
GUARD_HEADINGS = ("壊れたら気づく場所", "番人テスト")

# テスト関数名らしさ。短い識別子や 1 語の名前は拾わない。
TEST_NAME = re.compile(r"^[a-z_][a-z0-9_]*$")
MIN_LEN = 12
MIN_UNDERSCORES = 2

SIBLING_REPO_NAMES = ("clap-mml-play-server", "clap-mml-render-tui")


def rust_function_names(root: str) -> set[str]:
    names: set[str] = set()
    for dirpath, dirnames, filenames in os.walk(root):
        dirnames[:] = [d for d in dirnames if d not in ("target", ".git", "node_modules")]
        for filename in filenames:
            if not filename.endswith(".rs"):
                continue
            path = os.path.join(dirpath, filename)
            try:
                text = open(path, encoding="utf-8", errors="ignore").read()
            except OSError:
                continue
            names.update(re.findall(r"\bfn\s+([a-z_][a-z0-9_]*)", text))
    return names


def guard_section_text(markdown: str) -> str:
    """「壊れたら気づく場所」「番人テスト」の節だけを取り出す。"""
    out: list[str] = []
    inside = False
    heading_level = 0
    for line in markdown.splitlines():
        heading = re.match(r"^(#+)\s*(.*)$", line)
        if heading:
            level = len(heading.group(1))
            title = heading.group(2)
            if any(word in title for word in GUARD_HEADINGS):
                inside, heading_level = True, level
                continue
            if inside and level <= heading_level:
                inside = False
        if inside:
            out.append(line)
    return "\n".join(out)


def candidate_names(text: str) -> list[str]:
    """バッククォートの中からテスト関数名を拾う。

    2 つ絞り込みが要る:

    - **空白を含むバッククォートは見ない。** テスト名は単独か、パス・モジュールを
      前置して書く。空白が入っているのはコマンド行や散文で、そこに出る識別子
      （`--example parallel_instance_creation` など）は関数名ではない
    - **`::` や `/` で区切った末尾だけを見る。** 手前はディレクトリ名やモジュール名で、
      `app/src/tui/patch_role_report/tests.rs` の `patch_role_report` は関数ではない
    """
    found: list[str] = []
    for quoted in re.findall(r"`([^`]+)`", text):
        if re.search(r"\s", quoted):
            continue
        segments = [s for s in re.split(r"::|/", quoted) if s]
        if not segments:
            continue
        leaf = segments[-1].strip().rstrip("()").rstrip(",.")
        if not TEST_NAME.match(leaf):
            continue
        if len(leaf) < MIN_LEN or leaf.count("_") < MIN_UNDERSCORES:
            continue
        found.append(leaf)
    return found


def sibling_root(root: str) -> str | None:
    parent = os.path.dirname(os.path.abspath(root))
    here = os.path.basename(os.path.abspath(root))
    for name in SIBLING_REPO_NAMES:
        if name == here:
            continue
        candidate = os.path.join(parent, name)
        if os.path.isdir(os.path.join(candidate, "docs", "adr")):
            return candidate
    return None


def main() -> int:
    default_root = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", default=default_root, help="repo のルート")
    args = parser.parse_args()

    adr_dir = os.path.join(args.root, "docs", "adr")
    if not os.path.isdir(adr_dir):
        print(f"docs/adr が無い: {adr_dir}", file=sys.stderr)
        return 2

    here = rust_function_names(args.root)
    sibling = sibling_root(args.root)
    there = rust_function_names(sibling) if sibling else set()

    checked = 0
    unverified = 0
    missing: list[tuple[str, str]] = []
    for filename in sorted(os.listdir(adr_dir)):
        if not filename.endswith(".md"):
            continue
        text = open(os.path.join(adr_dir, filename), encoding="utf-8").read()
        for name in candidate_names(guard_section_text(text)):
            checked += 1
            if name in here or name in there:
                continue
            if sibling is None:
                unverified += 1
                continue
            missing.append((filename, name))

    for filename, name in missing:
        print(f"[NG] {filename}: `{name}` という関数がどちらの repo にも無い")

    scope = "この repo と隣の repo" if sibling else "この repo だけ"
    print(f"番人テスト名 {checked} 件を照合（{scope}）")
    if unverified:
        print(f"  うち {unverified} 件は隣の repo が見つからないので未確認（落とさない）")
    if missing:
        print(f"  {len(missing)} 件が見つからない。改名したなら ADR 側も直すこと")
        return 1
    print("  すべて実在する")
    return 0


if __name__ == "__main__":
    sys.exit(main())
