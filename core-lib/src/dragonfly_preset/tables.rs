//! Dragonfly Reverb 3.2.10 の組み込み preset 表（plugin の `DistrhoPluginInfo.h` から生成）。
//! `values` は `symbols` と同じ順。Plate の `algorithm` は 0=nrev, 1=nrevb, 2=strev。
//! Early Reflections は preset を持たず、`program` param の選択肢を preset として扱う。

use super::{DragonflyPlugin, DragonflyPreset};

#[rustfmt::skip]
pub(super) const HALL: DragonflyPlugin = DragonflyPlugin {
    plugin_id: "michaelwillis.dragonfly.hall",
    name: "Dragonfly Hall Reverb",
    file_stem: "DragonflyHallReverb",
    has_preset_state: true,
    symbols: &[
        "dry_level",
        "early_level",
        "late_level",
        "size",
        "width",
        "delay",
        "diffuse",
        "low_cut",
        "low_xo",
        "low_mult",
        "high_cut",
        "high_xo",
        "high_mult",
        "spin",
        "wander",
        "decay",
        "early_send",
        "modulation",
    ],
    presets: &[
        DragonflyPreset {
            name: "Bright Room",
            values: &[80.0, 10.0, 20.0, 10.0, 90.0, 4.0, 90.0, 4.0, 500.0, 0.80, 16000.0, 7900.0, 0.75, 1.0, 25.0, 0.6, 20.0, 30.0],
        },
        DragonflyPreset {
            name: "Clear Room",
            values: &[80.0, 10.0, 20.0, 10.0, 90.0, 4.0, 90.0, 4.0, 500.0, 0.90, 13000.0, 5800.0, 0.50, 1.0, 25.0, 0.6, 20.0, 30.0],
        },
        DragonflyPreset {
            name: "Dark Room",
            values: &[80.0, 10.0, 20.0, 10.0, 90.0, 4.0, 50.0, 4.0, 500.0, 1.20, 7300.0, 4900.0, 0.35, 1.0, 25.0, 0.7, 20.0, 30.0],
        },
        DragonflyPreset {
            name: "Small Chamber",
            values: &[80.0, 10.0, 20.0, 16.0, 80.0, 8.0, 70.0, 4.0, 500.0, 1.10, 8200.0, 5500.0, 0.35, 1.2, 10.0, 0.8, 20.0, 20.0],
        },
        DragonflyPreset {
            name: "Large Chamber",
            values: &[80.0, 10.0, 20.0, 20.0, 80.0, 8.0, 90.0, 4.0, 500.0, 1.30, 7000.0, 4900.0, 0.25, 1.8, 12.0, 1.0, 20.0, 20.0],
        },
        DragonflyPreset {
            name: "Acoustic Studio",
            values: &[80.0, 10.0, 20.0, 12.0, 90.0, 8.0, 75.0, 4.0, 450.0, 1.50, 7600.0, 4900.0, 0.80, 2.5, 7.0, 0.8, 20.0, 20.0],
        },
        DragonflyPreset {
            name: "Electric Studio",
            values: &[80.0, 10.0, 20.0, 12.0, 90.0, 6.0, 45.0, 4.0, 250.0, 1.25, 7600.0, 5800.0, 0.70, 2.5, 7.0, 0.9, 20.0, 30.0],
        },
        DragonflyPreset {
            name: "Percussion Studio",
            values: &[80.0, 10.0, 20.0, 12.0, 90.0, 6.0, 30.0, 20.0, 200.0, 1.75, 5800.0, 5200.0, 0.45, 2.5, 7.0, 0.7, 20.0, 10.0],
        },
        DragonflyPreset {
            name: "Piano Studio",
            values: &[80.0, 10.0, 20.0, 12.0, 80.0, 8.0, 40.0, 20.0, 600.0, 1.50, 8200.0, 5800.0, 0.50, 2.8, 10.0, 0.7, 20.0, 15.0],
        },
        DragonflyPreset {
            name: "Vocal Studio",
            values: &[80.0, 10.0, 20.0, 12.0, 90.0, 0.0, 60.0, 4.0, 400.0, 1.20, 5800.0, 5200.0, 0.40, 2.5, 7.0, 0.8, 20.0, 10.0],
        },
        DragonflyPreset {
            name: "Small Bright Hall",
            values: &[80.0, 10.0, 20.0, 24.0, 80.0, 12.0, 90.0, 4.0, 400.0, 1.10, 11200.0, 6250.0, 0.75, 2.5, 13.0, 1.3, 20.0, 15.0],
        },
        DragonflyPreset {
            name: "Small Clear Hall",
            values: &[80.0, 10.0, 20.0, 24.0, 100.0, 4.0, 90.0, 4.0, 500.0, 1.30, 7600.0, 5500.0, 0.50, 3.3, 15.0, 1.3, 20.0, 15.0],
        },
        DragonflyPreset {
            name: "Small Dark Hall",
            values: &[80.0, 10.0, 20.0, 24.0, 100.0, 12.0, 60.0, 4.0, 500.0, 1.50, 5800.0, 4000.0, 0.35, 2.5, 10.0, 1.5, 20.0, 15.0],
        },
        DragonflyPreset {
            name: "Small Percussion Hall",
            values: &[80.0, 10.0, 20.0, 24.0, 80.0, 12.0, 40.0, 20.0, 250.0, 2.00, 5200.0, 4000.0, 0.35, 2.0, 13.0, 1.1, 20.0, 10.0],
        },
        DragonflyPreset {
            name: "Small Vocal Hall",
            values: &[80.0, 10.0, 20.0, 24.0, 80.0, 4.0, 60.0, 4.0, 500.0, 1.25, 6250.0, 5200.0, 0.35, 3.1, 15.0, 1.2, 20.0, 10.0],
        },
        DragonflyPreset {
            name: "Medium Bright Hall",
            values: &[80.0, 10.0, 20.0, 30.0, 100.0, 18.0, 90.0, 4.0, 400.0, 1.25, 10000.0, 6400.0, 0.60, 2.9, 15.0, 1.6, 20.0, 15.0],
        },
        DragonflyPreset {
            name: "Medium Clear Hall",
            values: &[80.0, 10.0, 20.0, 30.0, 100.0, 8.0, 90.0, 4.0, 500.0, 1.50, 7600.0, 5500.0, 0.50, 2.9, 15.0, 1.7, 20.0, 15.0],
        },
        DragonflyPreset {
            name: "Medium Dark Hall",
            values: &[80.0, 10.0, 20.0, 30.0, 100.0, 18.0, 60.0, 4.0, 500.0, 1.75, 5800.0, 4000.0, 0.40, 2.9, 15.0, 1.8, 20.0, 15.0],
        },
        DragonflyPreset {
            name: "Medium Percussion Hall",
            values: &[80.0, 10.0, 20.0, 30.0, 80.0, 12.0, 40.0, 20.0, 300.0, 2.00, 5200.0, 4000.0, 0.35, 2.0, 12.0, 1.2, 20.0, 10.0],
        },
        DragonflyPreset {
            name: "Medium Vocal Hall",
            values: &[80.0, 10.0, 20.0, 32.0, 80.0, 8.0, 75.0, 4.0, 600.0, 1.50, 5800.0, 5200.0, 0.40, 2.8, 16.0, 1.3, 20.0, 10.0],
        },
        DragonflyPreset {
            name: "Large Bright Hall",
            values: &[80.0, 10.0, 20.0, 40.0, 100.0, 20.0, 90.0, 4.0, 400.0, 1.50, 8200.0, 5800.0, 0.50, 2.1, 20.0, 2.5, 20.0, 15.0],
        },
        DragonflyPreset {
            name: "Large Clear Hall",
            values: &[80.0, 10.0, 20.0, 40.0, 100.0, 12.0, 80.0, 4.0, 550.0, 2.00, 8200.0, 5200.0, 0.40, 2.1, 20.0, 2.8, 20.0, 15.0],
        },
        DragonflyPreset {
            name: "Large Dark Hall",
            values: &[80.0, 10.0, 20.0, 40.0, 100.0, 20.0, 60.0, 4.0, 600.0, 2.50, 6250.0, 2800.0, 0.20, 2.1, 20.0, 3.0, 20.0, 15.0],
        },
        DragonflyPreset {
            name: "Large Vocal Hall",
            values: &[80.0, 10.0, 20.0, 40.0, 80.0, 12.0, 80.0, 4.0, 700.0, 2.25, 6250.0, 4600.0, 0.30, 2.1, 17.0, 2.4, 20.0, 10.0],
        },
        DragonflyPreset {
            name: "Great Hall",
            values: &[80.0, 10.0, 20.0, 50.0, 90.0, 20.0, 95.0, 4.0, 750.0, 2.50, 5500.0, 4000.0, 0.30, 2.6, 22.0, 3.8, 20.0, 15.0],
        },
    ],
};

#[rustfmt::skip]
pub(super) const ROOM: DragonflyPlugin = DragonflyPlugin {
    plugin_id: "michaelwillis.dragonfly.room",
    name: "Dragonfly Room Reverb",
    file_stem: "DragonflyRoomReverb",
    has_preset_state: true,
    symbols: &[
        "dry_level",
        "early_level",
        "early_send",
        "late_level",
        "size",
        "width",
        "predelay",
        "decay",
        "diffuse",
        "spin",
        "wander",
        "in_high_cut",
        "early_damp",
        "late_damp",
        "low_boost",
        "boost_freq",
        "in_low_cut",
    ],
    presets: &[
        DragonflyPreset {
            name: "Small Bright Room",
            values: &[80.0, 10.0, 20.0, 20.0, 8.0, 90.0, 4.0, 0.2, 60.0, 0.4, 40.0, 16000.0, 16000.0, 16000.0, 20.0, 600.0, 4.0],
        },
        DragonflyPreset {
            name: "Small Clear Room",
            values: &[80.0, 10.0, 20.0, 20.0, 8.0, 90.0, 4.0, 0.2, 60.0, 0.8, 40.0, 16000.0, 11200.0, 10000.0, 40.0, 600.0, 4.0],
        },
        DragonflyPreset {
            name: "Small Dark Room",
            values: &[80.0, 10.0, 20.0, 20.0, 8.0, 90.0, 4.0, 0.3, 70.0, 1.6, 20.0, 16000.0, 6400.0, 5500.0, 60.0, 1000.0, 4.0],
        },
        DragonflyPreset {
            name: "Small Drum Room",
            values: &[80.0, 10.0, 20.0, 20.0, 9.0, 90.0, 8.0, 0.2, 24.0, 2.1, 10.0, 16000.0, 8200.0, 7000.0, 40.0, 400.0, 4.0],
        },
        DragonflyPreset {
            name: "Small Vocal Room",
            values: &[80.0, 10.0, 20.0, 20.0, 8.0, 90.0, 0.0, 0.3, 86.0, 2.4, 12.0, 16000.0, 7600.0, 6400.0, 20.0, 400.0, 4.0],
        },
        DragonflyPreset {
            name: "Medium Bright Room",
            values: &[80.0, 10.0, 20.0, 20.0, 12.0, 100.0, 8.0, 0.4, 60.0, 0.4, 40.0, 16000.0, 16000.0, 14000.0, 25.0, 600.0, 4.0],
        },
        DragonflyPreset {
            name: "Medium Clear Room",
            values: &[80.0, 10.0, 20.0, 20.0, 12.0, 100.0, 8.0, 0.4, 70.0, 0.8, 40.0, 16000.0, 10000.0, 9400.0, 50.0, 600.0, 4.0],
        },
        DragonflyPreset {
            name: "Medium Dark Room",
            values: &[80.0, 10.0, 20.0, 20.0, 12.0, 100.0, 8.0, 0.5, 70.0, 1.6, 20.0, 16000.0, 5800.0, 4600.0, 70.0, 1000.0, 4.0],
        },
        DragonflyPreset {
            name: "Medium Drum Room",
            values: &[80.0, 10.0, 20.0, 20.0, 12.0, 100.0, 12.0, 0.4, 32.0, 2.4, 10.0, 16000.0, 8000.0, 6000.0, 50.0, 300.0, 4.0],
        },
        DragonflyPreset {
            name: "Medium Vocal Room",
            values: &[80.0, 10.0, 20.0, 20.0, 12.0, 100.0, 2.0, 0.6, 92.0, 2.7, 12.0, 16000.0, 8000.0, 6000.0, 25.0, 400.0, 4.0],
        },
        DragonflyPreset {
            name: "Large Bright Room",
            values: &[80.0, 10.0, 20.0, 20.0, 15.0, 100.0, 12.0, 0.7, 70.0, 0.4, 40.0, 16000.0, 16000.0, 14000.0, 30.0, 600.0, 4.0],
        },
        DragonflyPreset {
            name: "Large Clear Room",
            values: &[80.0, 10.0, 20.0, 20.0, 15.0, 100.0, 12.0, 0.7, 80.0, 0.4, 40.0, 16000.0, 9400.0, 8500.0, 60.0, 600.0, 4.0],
        },
        DragonflyPreset {
            name: "Large Dark Room",
            values: &[80.0, 10.0, 20.0, 20.0, 15.0, 100.0, 12.0, 0.8, 80.0, 1.6, 20.0, 16000.0, 5200.0, 4000.0, 80.0, 1000.0, 4.0],
        },
        DragonflyPreset {
            name: "Large Drum Room",
            values: &[80.0, 10.0, 20.0, 20.0, 14.0, 100.0, 16.0, 0.7, 40.0, 2.7, 10.0, 16000.0, 8000.0, 5000.0, 60.0, 300.0, 4.0],
        },
        DragonflyPreset {
            name: "Large Vocal Room",
            values: &[80.0, 10.0, 20.0, 20.0, 15.0, 100.0, 4.0, 1.0, 98.0, 3.1, 12.0, 16000.0, 7000.0, 5000.0, 30.0, 400.0, 4.0],
        },
        DragonflyPreset {
            name: "Bright Hall",
            values: &[80.0, 10.0, 20.0, 20.0, 24.0, 100.0, 12.0, 1.6, 78.0, 0.4, 40.0, 16000.0, 16000.0, 14000.0, 30.0, 600.0, 4.0],
        },
        DragonflyPreset {
            name: "Clear Hall",
            values: &[80.0, 10.0, 20.0, 20.0, 24.0, 100.0, 12.0, 1.6, 84.0, 0.4, 40.0, 16000.0, 14000.0, 12000.0, 60.0, 600.0, 4.0],
        },
        DragonflyPreset {
            name: "Dark Hall",
            values: &[80.0, 10.0, 20.0, 20.0, 24.0, 100.0, 12.0, 1.9, 80.0, 1.6, 20.0, 16000.0, 8000.0, 6000.0, 80.0, 1000.0, 4.0],
        },
        DragonflyPreset {
            name: "Percussion Hall",
            values: &[80.0, 10.0, 20.0, 20.0, 20.0, 100.0, 16.0, 1.5, 40.0, 2.7, 10.0, 16000.0, 7000.0, 5000.0, 60.0, 300.0, 4.0],
        },
        DragonflyPreset {
            name: "Vocal Hall",
            values: &[80.0, 10.0, 20.0, 20.0, 25.0, 100.0, 4.0, 2.0, 98.0, 3.1, 12.0, 16000.0, 6000.0, 5000.0, 30.0, 400.0, 4.0],
        },
        DragonflyPreset {
            name: "Bright Plate",
            values: &[80.0, 10.0, 20.0, 20.0, 10.0, 100.0, 0.0, 1.8, 78.0, 1.6, 40.0, 16000.0, 16000.0, 15000.0, 10.0, 1000.0, 4.0],
        },
        DragonflyPreset {
            name: "Dark Plate",
            values: &[80.0, 10.0, 20.0, 20.0, 10.0, 100.0, 0.0, 2.0, 78.0, 0.4, 20.0, 16000.0, 5200.0, 4000.0, 40.0, 500.0, 4.0],
        },
        DragonflyPreset {
            name: "Brick Wall",
            values: &[80.0, 10.0, 20.0, 20.0, 20.0, 80.0, 80.0, 0.4, 12.0, 0.2, 5.0, 16000.0, 16000.0, 15000.0, 10.0, 200.0, 4.0],
        },
        DragonflyPreset {
            name: "Echo Chamber",
            values: &[80.0, 10.0, 20.0, 20.0, 12.0, 120.0, 10.0, 4.0, 24.0, 2.3, 80.0, 16000.0, 12000.0, 9000.0, 20.0, 500.0, 4.0],
        },
        DragonflyPreset {
            name: "Long Tunnel",
            values: &[80.0, 10.0, 20.0, 20.0, 25.0, 50.0, 0.0, 8.0, 90.0, 0.4, 10.0, 16000.0, 8000.0, 6000.0, 50.0, 500.0, 4.0],
        },
    ],
};

#[rustfmt::skip]
pub(super) const PLATE: DragonflyPlugin = DragonflyPlugin {
    plugin_id: "michaelwillis.dragonfly.plate",
    name: "Dragonfly Plate Reverb",
    file_stem: "DragonflyPlateReverb",
    has_preset_state: true,
    symbols: &[
        "dry_level",
        "early_level",
        "algorithm",
        "width",
        "predelay",
        "decay",
        "low_cut",
        "high_cut",
        "early_damp",
    ],
    presets: &[
        DragonflyPreset {
            name: "Abrupt Plate",
            values: &[80.0, 20.0, 1.0, 100.0, 20.0, 0.2, 50.0, 10000.0, 7000.0],
        },
        DragonflyPreset {
            name: "Bright Plate",
            values: &[80.0, 20.0, 1.0, 100.0, 0.0, 0.4, 200.0, 16000.0, 13000.0],
        },
        DragonflyPreset {
            name: "Clear Plate",
            values: &[80.0, 20.0, 1.0, 100.0, 0.0, 0.6, 100.0, 13000.0, 7000.0],
        },
        DragonflyPreset {
            name: "Dark Plate",
            values: &[80.0, 20.0, 1.0, 100.0, 0.0, 0.8, 50.0, 7000.0, 4000.0],
        },
        DragonflyPreset {
            name: "Foil Tray",
            values: &[80.0, 20.0, 0.0, 50.0, 0.0, 0.3, 200.0, 16000.0, 13000.0],
        },
        DragonflyPreset {
            name: "Metal Roof",
            values: &[80.0, 20.0, 0.0, 120.0, 20.0, 0.5, 100.0, 13000.0, 10000.0],
        },
        DragonflyPreset {
            name: "Narrow Tank",
            values: &[80.0, 20.0, 2.0, 60.0, 10.0, 0.6, 50.0, 10000.0, 7000.0],
        },
        DragonflyPreset {
            name: "Phat Tank",
            values: &[80.0, 20.0, 2.0, 150.0, 10.0, 1.0, 50.0, 10000.0, 4000.0],
        },
    ],
};

#[rustfmt::skip]
pub(super) const EARLY_REFLECTIONS: DragonflyPlugin = DragonflyPlugin {
    plugin_id: "michaelwillis.dragonfly.early",
    name: "Dragonfly Early Reflections",
    file_stem: "DragonflyEarlyReflections",
    has_preset_state: false,
    symbols: &[
        "program",
    ],
    presets: &[
        DragonflyPreset {
            name: "Abrupt Echo",
            values: &[0.0],
        },
        DragonflyPreset {
            name: "Backstage Pass",
            values: &[1.0],
        },
        DragonflyPreset {
            name: "Concert Venue",
            values: &[2.0],
        },
        DragonflyPreset {
            name: "Damaged Goods",
            values: &[3.0],
        },
        DragonflyPreset {
            name: "Elevator Pitch",
            values: &[4.0],
        },
        DragonflyPreset {
            name: "Floor Thirteen",
            values: &[5.0],
        },
        DragonflyPreset {
            name: "Garage Band",
            values: &[6.0],
        },
        DragonflyPreset {
            name: "Home Studio",
            values: &[7.0],
        },
    ],
};
