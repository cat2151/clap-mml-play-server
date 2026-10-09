//! Floe の Lua 5.4 script を実行し、関数・loop・dofile を反映した region を受け取る。

use std::{
    cell::RefCell,
    path::{Component, Path},
    rc::Rc,
    time::{Duration, Instant},
};

use anyhow::{Context, Result};
use mlua::{AnyUserData, HookTriggers, Lua, LuaOptions, StdLib, Table, UserData, VmState};

use super::library_regions::{library_hash, Library, NamedKeyRange, Region};
use crate::drum_kit_note::sample_name;

#[derive(Clone)]
struct LibraryHandle(Rc<RefCell<Library>>);
impl UserData for LibraryHandle {}

#[derive(Clone)]
struct InstrumentHandle {
    library: LibraryHandle,
    id: String,
}
impl UserData for InstrumentHandle {}

pub(super) fn evaluate(path: &Path) -> Result<Library> {
    let lua = Lua::new_with(
        StdLib::TABLE | StdLib::STRING | StdLib::MATH | StdLib::UTF8,
        LuaOptions::default(),
    )?;
    lua.set_memory_limit(64 * 1024 * 1024)?;
    let deadline = Instant::now() + Duration::from_secs(5);
    lua.set_hook(
        HookTriggers::new().every_nth_instruction(10_000),
        move |_, _| {
            if Instant::now() > deadline {
                return Err(mlua::Error::runtime("Floe Lua 評価が 5 秒を超えた"));
            }
            Ok(VmState::Continue)
        },
    )?;
    let api = lua.create_table()?;
    api.set(
        "new_library",
        lua.create_function(|_, params: Table| {
            let name = params.get::<String>("name")?;
            let author = params.get::<String>("author")?;
            let id = params
                .get::<Option<String>>("id")?
                .unwrap_or_else(|| format!("{name} - {author}"));
            Ok(LibraryHandle(Rc::new(RefCell::new(Library {
                id: library_hash(&id),
                ..Default::default()
            }))))
        })?,
    )?;
    api.set(
        "new_instrument",
        lua.create_function(|_, (library, params): (AnyUserData, Table)| {
            let library = library.borrow::<LibraryHandle>()?.clone();
            let name = params.get::<String>("name")?;
            let id = params.get::<Option<String>>("id")?.unwrap_or(name);
            if library
                .0
                .borrow_mut()
                .instruments
                .insert(id.clone(), Vec::new())
                .is_some()
            {
                return Err(mlua::Error::runtime(format!(
                    "Floe instrument ID が重複: {id}"
                )));
            }
            Ok(InstrumentHandle { library, id })
        })?,
    )?;
    api.set(
        "add_region",
        lua.create_function(|_, (instrument, params): (AnyUserData, Table)| {
            let instrument = instrument.borrow::<InstrumentHandle>()?;
            // Floe copies the region at add_region; later Lua mutations must not alter it.
            let region = region(&params)?;
            instrument
                .library
                .0
                .borrow_mut()
                .instruments
                .get_mut(&instrument.id)
                .ok_or_else(|| mlua::Error::runtime("Floe instrument が無い"))?
                .push(region);
            Ok(())
        })?,
    )?;
    api.set(
        "add_named_key_range",
        lua.create_function(|_, (instrument, params): (AnyUserData, Table)| {
            let instrument = instrument.borrow::<InstrumentHandle>()?;
            let name = params.get::<String>("name")?;
            let range = params.get::<Table>("key_range")?;
            let (low, end) = key_range(&range)?;
            instrument
                .library
                .0
                .borrow_mut()
                .named_key_ranges
                .entry(instrument.id.clone())
                .or_default()
                .push(NamedKeyRange { name, low, end });
            Ok(())
        })?,
    )?;
    for name in ["set_attribution_requirement", "set_required_floe_version"] {
        api.set(name, lua.create_function(|_, _: mlua::MultiValue| Ok(()))?)?;
    }
    api.set(
        "add_ir",
        lua.create_function(|lua, _: mlua::MultiValue| lua.create_table())?,
    )?;
    for (name, steps) in [
        ("midi_range_to_hundred_range", 99.0_f64),
        ("midi_range_to_thousand_range", 999.0),
    ] {
        api.set(
            name,
            lua.create_function(move |lua, range: Table| {
                let lo = range.get::<i32>(1)?;
                let hi = range.get::<i32>(2)?;
                if !(0..=127).contains(&lo) || !(0..=127).contains(&hi) || lo >= hi {
                    return Err(mlua::Error::runtime("Floe MIDI velocity range が不正"));
                }
                let result = lua.create_table()?;
                result.set(
                    1,
                    ((f64::from(lo.max(1) - 1) / 126.0) * steps).round() as u16,
                )?;
                result.set(
                    2,
                    ((f64::from(hi) / 126.0) * steps).min(steps + 1.0).round() as u16,
                )?;
                Ok(result)
            })?,
        )?;
    }
    lua.globals().set("floe", api)?;
    lua.load(
        r#"
        function floe.extend_table(base, t)
            t = t or {}
            for key, value in pairs(base) do
                if type(value) == "table" then t[key] = floe.extend_table(value, t[key])
                elseif t[key] == nil then t[key] = value end
            end
            return t
        end
    "#,
    )
    .exec()?;
    let root = path
        .parent()
        .context("Floe library の親 directory が無い")?
        .to_owned();
    lua.globals().set(
        "dofile",
        lua.create_function(move |lua, relative: String| {
            let relative = relative.replace('\\', "/");
            let relative = Path::new(&relative);
            if relative.is_absolute()
                || relative
                    .components()
                    .any(|c| matches!(c, Component::ParentDir | Component::Prefix(_)))
            {
                return Err(mlua::Error::runtime(
                    "Floe dofile は library 内の相対 path のみ",
                ));
            }
            let text =
                std::fs::read_to_string(root.join(relative)).map_err(mlua::Error::external)?;
            lua.load(text.trim_start_matches('\u{feff}'))
                .set_name(relative.to_string_lossy())
                .eval::<mlua::MultiValue>()
        })?,
    )?;
    lua.globals().set(
        "loadfile",
        lua.create_function(|_, _: mlua::MultiValue| -> mlua::Result<()> {
            Err(mlua::Error::runtime(
                "Floe loadfile は非対応。dofile を使用すること",
            ))
        })?,
    )?;
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("Floe library を読めない: {}", path.display()))?;
    let returned: AnyUserData = lua
        .load(text.trim_start_matches('\u{feff}'))
        .set_name(path.to_string_lossy())
        .eval()
        .with_context(|| format!("Floe library の Lua 評価失敗: {}", path.display()))?;
    let handle = returned.borrow::<LibraryHandle>()?;
    let mut library = std::mem::take(&mut *handle.0.borrow_mut());
    if library.instruments.values().any(Vec::is_empty) {
        anyhow::bail!(
            "Floe library の instrument に region が無い: {}",
            path.display()
        );
    }
    library.finish_mapping();
    Ok(library)
}

fn region(params: &Table) -> mlua::Result<Region> {
    let path = params.get::<String>("path")?;
    let root = params.get::<u8>("root_key")?;
    if root > 127 {
        return Err(mlua::Error::runtime("Floe root_key が不正"));
    }
    let trigger = params.get::<Option<Table>>("trigger_criteria")?;
    let range = trigger
        .as_ref()
        .map(|t| t.get::<Option<Table>>("key_range"))
        .transpose()?
        .flatten();
    let (low, end) = match range {
        Some(range) => key_range(&range)?,
        None => (0, 128),
    };
    let event = trigger
        .as_ref()
        .map(|t| t.get::<Option<String>>("trigger_event"))
        .transpose()?
        .flatten();
    let note_on = match event.as_deref() {
        None | Some("note-on") => true,
        Some("note-off") => false,
        Some(event) => {
            return Err(mlua::Error::runtime(format!(
                "Floe trigger_event が不正: {event}"
            )))
        }
    };
    let auto_map = trigger
        .as_ref()
        .map(|t| t.get::<Option<String>>("auto_map_key_range_group"))
        .transpose()?
        .flatten();
    Ok(Region {
        name: sample_name(&path).to_string(),
        root,
        low,
        end,
        note_on,
        auto_map,
    })
}

/// Floe の `{low, end}`。`end` は含まない。
fn key_range(range: &Table) -> mlua::Result<(u8, u8)> {
    let (low, end) = (range.get::<u8>(1)?, range.get::<u8>(2)?);
    if low > 127 || end > 128 || end == 0 {
        return Err(mlua::Error::runtime("Floe key_range が不正"));
    }
    Ok((low, end))
}
