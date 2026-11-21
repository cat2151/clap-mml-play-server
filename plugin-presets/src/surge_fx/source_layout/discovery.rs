use super::expression::{Environment, Value};
use super::syntax::{field, nodes, text, Cpp};
use anyhow::{ensure, Context, Result};

pub struct EffectClass {
    pub fx_type: i32,
    pub display_name: String,
    pub class_name: String,
}

pub fn storage_metadata(storage: &str) -> Result<(Environment, Vec<String>)> {
    let mut env = Environment::default();
    env.collect(storage)?;
    let Value::Array(names) = env
        .0
        .get("fx_type_names")
        .context("missing fx_type_names")?
    else {
        anyhow::bail!("fx_type_names is not an array")
    };
    let names = names
        .iter()
        .map(Value::string)
        .collect::<Result<Vec<_>>>()?;
    ensure!(
        env.integer_name("n_fx_types")? == names.len() as i64,
        "effect type/name counts differ"
    );
    Ok((env, names))
}

pub fn effect_classes(storage: &str, effect_base: &str) -> Result<Vec<EffectClass>> {
    let (env, names) = storage_metadata(storage)?;
    let cpp = Cpp::parse(effect_base)?;
    let mut effects = Vec::new();
    for case in nodes(cpp.function("spawn_effect")?)
        .into_iter()
        .filter(|n| n.kind() == "case_statement")
    {
        let Some(value) = case.child_by_field_name("value") else {
            continue;
        };
        let fx_type = i32::try_from(env.eval(value, effect_base)?.integer()?)?;
        let constructors: Vec<_> = nodes(case)
            .into_iter()
            .filter(|n| n.kind() == "new_expression")
            .collect();
        ensure!(
            constructors.len() == 1,
            "unsupported effect factory case {fx_type}"
        );
        let ty = text(field(constructors[0], "type")?, effect_base);
        let class_name = ty
            .split('<')
            .next()
            .context("missing effect class")?
            .trim()
            .to_owned();
        effects.push(EffectClass {
            fx_type,
            display_name: names
                .get(usize::try_from(fx_type)?)
                .context("effect type out of range")?
                .clone(),
            class_name,
        });
    }
    effects.sort_by_key(|e| e.fx_type);
    ensure!(!effects.is_empty(), "no effects discovered");
    Ok(effects)
}
