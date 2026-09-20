#!/usr/bin/env python3
"""Surge XT のソースから、effect ごとの parameter layout 表を生成する。

生成先: core-lib/src/surge_fx_preset/param_layout.rs

Surge XT Effects の plugin state は parameter を GUI の並び（`posy_offset` で決まる）
で流す一方、`.srgfx` preset は storage の並び（enum 順）で持つ。両者の対応は
各 effect の `init_ctrltypes()` にしか書かれていないので、ここで写し取る。

対象の版は plugin の版に合わせて固定する（下の TAG / SST_EFFECTS_SHA）。

使い方:
    python scripts/gen_surge_fx_param_layout.py [--cache-dir DIR]
"""

from __future__ import annotations

import argparse
import pathlib
import re
import sys
import tempfile
import urllib.request

TAG = "release_xt_1.3.4"
SST_EFFECTS_SHA = "43a54d94370986f7fe92931493435f2a8b67603f"
SURGE_RAW = f"https://raw.githubusercontent.com/surge-synthesizer/surge/{TAG}/src/common/dsp/effects/"
SST_RAW = f"https://raw.githubusercontent.com/surge-synthesizer/sst-effects/{SST_EFFECTS_SHA}/include/sst/effects/"
N_FX_PARAMS = 12

# (fx_type, 表示名, ctrltypes を読む cpp, enum を読む header, sst-effects の header または None)
EFFECTS = [
    (1, "Delay", "DelayEffect.cpp", "sst:Delay.h", "Delay.h"),
    (2, "Reverb 1", "Reverb1Effect.cpp", "sst:Reverb1.h", "Reverb1.h"),
    (3, "Phaser", "PhaserEffect.cpp", "sst:Phaser.h", "Phaser.h"),
    (4, "Rotary Speaker", "RotarySpeakerEffect.cpp", "RotarySpeakerEffect.h", None),
    (5, "Distortion", "DistortionEffect.cpp", "DistortionEffect.h", None),
    (6, "EQ", "ParametricEQ3BandEffect.cpp", "ParametricEQ3BandEffect.h", None),
    (7, "Frequency Shifter", "FrequencyShifterEffect.cpp", "FrequencyShifterEffect.h", None),
    (8, "Conditioner", "ConditionerEffect.cpp", "ConditionerEffect.h", None),
    (9, "Chorus", "ChorusEffectImpl.h", "ChorusEffect.h", None),
    (10, "Vocoder", "VocoderEffect.cpp", "VocoderEffect.h", None),
    (11, "Reverb 2", "Reverb2Effect.cpp", "Reverb2Effect.h", None),
    (12, "Flanger", "FlangerEffect.cpp", "sst:Flanger.h", "Flanger.h"),
    (13, "Ring Modulator", "RingModulatorEffect.cpp", "RingModulatorEffect.h", None),
    (14, "Airwindows", "airwindows/AirWindowsEffect.cpp", None, None),
    (15, "Neuron", "chowdsp/NeuronEffect.cpp", "chowdsp/NeuronEffect.h", None),
    (16, "Graphic EQ", "GraphicEQ11BandEffect.cpp", "GraphicEQ11BandEffect.h", None),
    (17, "Resonator", "ResonatorEffect.cpp", "ResonatorEffect.h", None),
    (18, "CHOW", "chowdsp/CHOWEffect.cpp", "chowdsp/CHOWEffect.h", None),
    (19, "Exciter", "chowdsp/ExciterEffect.cpp", "chowdsp/ExciterEffect.h", None),
    (20, "Ensemble", "BBDEnsembleEffect.cpp", "BBDEnsembleEffect.h", None),
    (21, "Combulator", "CombulatorEffect.cpp", "CombulatorEffect.h", None),
    (22, "Nimbus", "NimbusEffect.cpp", "NimbusEffect.h", None),
    (23, "Tape", "chowdsp/TapeEffect.cpp", "chowdsp/TapeEffect.h", None),
    (24, "Treemonster", "TreemonsterEffect.cpp", "TreemonsterEffect.h", None),
    (25, "Waveshaper", "WaveShaperEffect.cpp", "WaveShaperEffect.h", None),
    (26, "Mid-Side Tool", "MSToolEffect.cpp", "MSToolEffect.h", None),
    (27, "Spring Reverb", "chowdsp/SpringReverbEffect.cpp", "chowdsp/SpringReverbEffect.h", None),
    (28, "Bonsai", "BonsaiEffect.cpp", "sst:Bonsai.h", "Bonsai.h"),
]

# posy_offset がループや変数で決まる effect は、ソースを読んだ結果をここに直接書く。
# 値は storage index 順。
MANUAL_POSY = {
    6: [1, 1, 1, 3, 3, 3, 5, 5, 5, 7, 7],  # 1 + 2 * (i / 3)
    11: [1, 3, 3, 3, 3, 3, 5, 5, 7, 7],  # room_size 以降 +2, lf_damping 以降 +2, width 以降 +2
    13: [1, 1, 1, 1, 3, 3, 5, 5, 7],  # diode_fwdbias / lowcut / mix で +2
    14: [1] + [0] * 11,  # p0 だけ。p1.. は Effect::init_ctrltypes の既定 0
    16: [1] * 11 + [3],  # band は 1、gain は 3
    22: [1, 1, 3, 3, 3, 3, 3, 3, 5, 5, 7, 7],  # ypos が 2 ずつ進む
    25: [1, 1, 3, 3, 3, 5, 5, 7, 7],  # shaper / postlowcut / postboost で +2
}
# 名前がループで付く effect。
MANUAL_NAMES = {
    14: ["FX"] + [f"Param {i}" for i in range(11)],
    16: ["30 Hz", "60 Hz", "120 Hz", "250 Hz", "500 Hz", "1 kHz", "2 kHz", "4 kHz", "8 kHz", "12 kHz", "16 kHz", "Gain"],
}
# set_type がループで付く effect（storage index → 有効）。
MANUAL_ACTIVE = {
    14: [True] * 12,
    16: [True] * 12,
}


def fetch(cache: pathlib.Path, url: str) -> str:
    target = cache / url.replace("https://", "").replace("/", "_")
    if not target.exists():
        with urllib.request.urlopen(url) as response:
            target.write_bytes(response.read())
    return target.read_text(encoding="utf-8", errors="replace")


def strip_comments(source: str) -> str:
    source = re.sub(r"/\*.*?\*/", "", source, flags=re.S)
    return re.sub(r"//[^\n]*", "", source)


def parse_enum(source: str, first_member: str) -> dict[str, int]:
    """`first_member` で始まる enum を index 辞書にする。"""
    source = strip_comments(source)
    body = next(
        (m.group(1) for m in re.finditer(r"enum\s+\w+\s*\{([^}]*)\}", source, re.S)
         if re.search(r"\b" + re.escape(first_member) + r"\b", m.group(1))),
        None,
    )
    if body is None:
        raise SystemExit(f"enum with {first_member} not found")
    members: dict[str, int] = {}
    value = 0
    for raw in body.split(","):
        item = raw.strip()
        if not item:
            continue
        if "=" in item:
            name, number = (part.strip() for part in item.split("="))
            value = int(number)
        else:
            name = item
        members[name] = value
        value += 1
    return members


def init_ctrltypes_body(source: str) -> str:
    source = strip_comments(source)
    match = re.search(r"::init_ctrltypes\(\)\s*\{", source)
    if not match:
        raise SystemExit("init_ctrltypes not found")
    depth, index = 1, match.end()
    while depth and index < len(source):
        depth += {"{": 1, "}": -1}.get(source[index], 0)
        index += 1
    return source[match.end() : index]


def sst_names(source: str, members: dict[str, int]) -> dict[int, str]:
    names: dict[int, str] = {}
    source = strip_comments(source)
    for name, index in members.items():
        match = re.search(rf"case\s+{re.escape(name)}\s*:(.*?)(?=\bcase\s+\w+\s*:|\bdefault\s*:)", source, re.S)
        if match:
            named = re.search(r'withName\("([^"]*)"\)', match.group(1))
            if named:
                names[index] = named.group(1)
    return names


def build(cache: pathlib.Path) -> list[dict]:
    rows = []
    for fx_type, display, cpp, header, sst in EFFECTS:
        body = init_ctrltypes_body(fetch(cache, SURGE_RAW + cpp))
        members: dict[str, int] = {}
        if header:
            if header.startswith("sst:"):
                header_source = fetch(cache, SST_RAW + header[4:])
            else:
                header_source = fetch(cache, SURGE_RAW + header)
            first = re.search(r"fxdata->p\[(\w+)\]", body).group(1)
            members = parse_enum(header_source, first)
        posy = MANUAL_POSY.get(fx_type, [0] * N_FX_PARAMS)[:] + [0] * N_FX_PARAMS
        active = MANUAL_ACTIVE.get(fx_type, [False] * N_FX_PARAMS)[:] + [False] * N_FX_PARAMS
        names = MANUAL_NAMES.get(fx_type, [""] * N_FX_PARAMS)[:] + [""] * N_FX_PARAMS

        def index_of(token: str) -> int | None:
            # ループ変数（`p[i]`）は MANUAL_* で扱うので読み飛ばす。
            if token.isdigit():
                return int(token)
            return members.get(token)

        for token, ctrltype in re.findall(r"fxdata->p\[(\w+)\]\.set_type\((\w+)\)", body):
            if (index := index_of(token)) is not None:
                active[index] = ctrltype != "ct_none"
        for token, name in re.findall(r'fxdata->p\[(\w+)\]\.set_name\("([^"]*)"\)', body):
            if (index := index_of(token)) is not None:
                names[index] = name
        if fx_type not in MANUAL_POSY:
            for token, value in re.findall(r"fxdata->p\[(\w+)\]\.posy_offset\s*=\s*(-?\d+)", body):
                if (index := index_of(token)) is not None:
                    posy[index] = int(value)
        if sst:
            for index, name in sst_names(fetch(cache, SST_RAW + sst), members).items():
                names[index] = name
        rows.append(
            {
                "fx_type": fx_type,
                "display": display,
                "posy": posy[:N_FX_PARAMS],
                "active": active[:N_FX_PARAMS],
                "names": names[:N_FX_PARAMS],
            }
        )
    return rows


def rust_source(rows: list[dict]) -> str:
    lines = [
        "//! effect 種別ごとの parameter layout（storage 順の並びと GUI 側の並びの対応）。",
        "//!",
        "//! `scripts/gen_surge_fx_param_layout.py` が Surge XT のソースから生成する。",
        "//! 手で編集せず、スクリプトを直すこと。",
        "",
        "use super::SURGE_FX_PARAM_COUNT;",
        "",
        "/// 1 effect 種別ぶんの layout。配列はすべて storage index（`.srgfx` の `pN`）順。",
        "#[derive(Clone, Copy, Debug)]",
        "pub struct SurgeFxParamLayout {",
        "    pub fx_type: i32,",
        "    pub display_name: &'static str,",
        "    /// GUI の並びを決める offset。0 なら GUI の並びに参加しない。",
        "    pub posy_offset: [i32; SURGE_FX_PARAM_COUNT],",
        "    /// `ct_none` でない（値が流れる）parameter。",
        "    pub active: [bool; SURGE_FX_PARAM_COUNT],",
        "    /// plugin が `clap.params` で名乗る名前（group 名を除く）。",
        "    pub names: [&'static str; SURGE_FX_PARAM_COUNT],",
        "}",
        "",
        "pub const SURGE_FX_PARAM_LAYOUTS: &[SurgeFxParamLayout] = &[",
    ]
    for row in rows:
        lines.append("    SurgeFxParamLayout {")
        lines.append(f"        fx_type: {row['fx_type']},")
        lines.append(f"        display_name: \"{row['display']}\",")
        lines.append(f"        posy_offset: [{', '.join(str(v) for v in row['posy'])}],")
        lines.append(f"        active: [{', '.join('true' if v else 'false' for v in row['active'])}],")
        names = ", ".join('"' + name.replace('"', '\\"') + '"' for name in row["names"])
        lines.append(f"        names: [{names}],")
        lines.append("    },")
    lines.append("];")
    return "\n".join(lines) + "\n"


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--cache-dir", type=pathlib.Path, default=pathlib.Path(tempfile.gettempdir()) / "surge-src-cache")
    args = parser.parse_args()
    args.cache_dir.mkdir(parents=True, exist_ok=True)
    rows = build(args.cache_dir)
    out = pathlib.Path(__file__).resolve().parent.parent / "core-lib" / "src" / "surge_fx_preset" / "param_layout.rs"
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(rust_source(rows), encoding="utf-8", newline="\n")
    print(f"wrote {out} ({len(rows)} effects)", file=sys.stderr)


if __name__ == "__main__":
    main()
