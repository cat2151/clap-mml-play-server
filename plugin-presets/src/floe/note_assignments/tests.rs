use super::*;
use crate::floe::library_regions::library_hash;

fn fixture() -> PathBuf {
    let suffix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root =
        std::env::temp_dir().join(format!("cmrt_floe_notes_{}_{suffix}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    root
}

fn preset(root: &Path, layers: &[(&str, u8, u8, i16)]) -> PathBuf {
    let mut bytes = Vec::new();
    bytes.extend(0x2a491f93_u32.to_le_bytes());
    bytes.extend(30_u16.to_le_bytes());
    bytes.extend(0x020002_u32.to_le_bytes());
    for i in 0..3 {
        if let Some((id, _, _, _)) = layers.get(i) {
            bytes.push(1);
            bytes.extend(library_hash("Kit - Test").to_le_bytes());
            bytes.extend((id.len() as u32).to_le_bytes());
            bytes.extend(id.as_bytes());
        } else {
            bytes.push(0);
        }
        bytes.push(0); // velocity curve points.
        bytes.extend([0; 16 + 64 * 8 + 2]);
    }
    bytes.push(0); // tags.
    bytes.extend([0; 6]); // three empty strings.
    bytes.extend((layers.len() as u16 * 3).to_le_bytes());
    for (i, (_, low, high, transpose)) in layers.iter().enumerate() {
        for (param, value) in [
            (50, f32::from(*low)),
            (51, f32::from(*high)),
            (48, f32::from(*transpose)),
        ] {
            bytes.extend((160 * (i as u32 + 1) + param).to_le_bytes());
            bytes.extend(value.to_le_bytes());
        }
    }
    let path = root.join("kit.floe-preset");
    std::fs::write(&path, bytes).unwrap();
    path
}

#[test]
fn evaluated_functions_loops_includes_and_layer_bounds_keep_holes() {
    let root = fixture();
    std::fs::write(root.join("keys.lua"), "return {36, 42, 70}").unwrap();
    std::fs::write(root.join("floe.lua"), r#"
        local lib = floe.new_library({name="Kit", author="Test"})
        local a = floe.new_instrument(lib,{name="Ignored name", id="A"})
        local function add(key)
            floe.add_region(a,{path="sample.wav", root_key=key, trigger_criteria={key_range={key,key+1}}})
        end
        for _,key in ipairs(dofile("keys.lua")) do add(key); add(key) end
        floe.add_region(a,{path="off.wav",root_key=80,trigger_criteria={trigger_event="note-off",key_range={80,81}}})
        local b=floe.new_instrument(lib,{name="B"})
        floe.add_region(b,{path="wide.wav",root_key=60,trigger_criteria={key_range={60,64}}})
        return lib
    "#).unwrap();
    let path = preset(&root, &[("A", 35, 60, 0), ("B", 58, 59, 2)]);
    assert_eq!(
        notes_from_dirs(&path, std::slice::from_ref(&root)).unwrap(),
        vec![36, 42, 58, 59]
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn auto_mapping_is_completed_before_note_on_filtering() {
    let root = fixture();
    std::fs::write(root.join("test.floe.lua"), r#"
        local lib=floe.new_library({name="Kit", author="Test"})
        local inst=floe.new_instrument(lib,{name="A"})
        for _,root in ipairs({60,36}) do
            floe.add_region(inst,{path="sample.wav",root_key=root,trigger_criteria={
                auto_map_key_range_group="group",trigger_event=root==36 and "note-on" or "note-off"}})
        end
        return lib
    "#).unwrap();
    let path = preset(&root, &[("A", 30, 60, 0)]);
    assert_eq!(
        notes_from_dirs(&path, std::slice::from_ref(&root)).unwrap(),
        (30..=48).collect::<Vec<_>>()
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn unresolved_layer_and_lua_failure_do_not_return_successful_layers_subset() {
    let root = fixture();
    std::fs::write(
        root.join("floe.lua"),
        r#"
        local lib=floe.new_library({name="Kit",author="Test"})
        local inst=floe.new_instrument(lib,{name="A"})
        floe.add_region(inst,{path="sample.wav",root_key=36,trigger_criteria={key_range={36,37}}})
        return lib
    "#,
    )
    .unwrap();
    let path = preset(&root, &[("A", 0, 127, 0), ("Missing", 0, 127, 0)]);
    assert!(notes_from_dirs(&path, std::slice::from_ref(&root)).is_err());
    let path = preset(&root, &[("A", 0, 127, 0)]);
    std::fs::write(root.join("floe.lua"), "error('bad library')").unwrap();
    assert!(notes_from_dirs(&path, std::slice::from_ref(&root)).is_err());
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn equal_auto_map_roots_preserve_floes_note_on_off_order() {
    let root = fixture();
    std::fs::write(
        root.join("floe.lua"),
        r#"
        local lib=floe.new_library({name="Kit",author="Test"})
        local inst=floe.new_instrument(lib,{name="A"})
        for i=1,5 do
            floe.add_region(inst,{path="sample.wav",root_key=36,trigger_criteria={
                auto_map_key_range_group="group",trigger_event=i==2 and "note-on" or "note-off"}})
        end
        return lib
    "#,
    )
    .unwrap();
    let path = preset(&root, &[("A", 0, 127, 0)]);
    assert_eq!(
        notes_from_dirs(&path, std::slice::from_ref(&root)).unwrap(),
        (37..=127).collect::<Vec<_>>()
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
#[ignore = "set CMRT_TEST_FLOE_PRESET to a real installed Taiko preset"]
fn installed_taiko_matches_evaluated_note_assignments() {
    let path = std::env::var("CMRT_TEST_FLOE_PRESET").expect("real preset path");
    let notes = floe_note_assignments(Path::new(&path)).unwrap();
    let expected = vec![
        36, 37, 38, 39, 41, 42, 43, 44, 45, 46, 48, 49, 50, 51, 53, 54, 55, 56, 57, 58, 60, 61, 62,
        63, 65, 66, 67, 68, 69, 70,
    ];
    assert_eq!(notes, expected);
    eprintln!("{path}: {notes:?}");
}
