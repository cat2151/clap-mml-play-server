//! Official-source layout extraction. No effect-specific parameter values live here.
//!
//! Only the C++ constructs used by the supported release are interpreted. This is
//! not a C++ runtime: source code is never compiled or executed.

mod controls;
mod discovery;
mod expression;
mod syntax;

use super::SurgeFxParamLayout;
use anyhow::{ensure, Context, Result};
use expression::Environment;
use syntax::{children, field, nodes, text, Cpp};

pub use discovery::{effect_classes, EffectClass};

/// Extract one effect, including metadata delegated to sst-effects.
pub struct LayoutSources<'a> {
    pub implementation: &'a str,
    pub header: &'a str,
    pub sst_header: Option<&'a str>,
    pub storage: &'a str,
    pub effect_base: &'a str,
    pub parameter: &'a str,
}

pub fn extract_layout(
    fx_type: i32,
    display_name: String,
    sources: LayoutSources<'_>,
) -> Result<SurgeFxParamLayout> {
    let LayoutSources {
        implementation,
        header,
        sst_header,
        storage,
        effect_base,
        parameter,
    } = sources;
    let mut env = Environment::default();
    for source in [storage, header].into_iter().chain(sst_header) {
        env.collect(source)?;
    }
    let count = env.integer_name("n_fx_params")?;
    ensure!(
        count == super::SURGE_FX_PARAM_COUNT as i64,
        "n_fx_params changed: {count}"
    );
    let mut result = SurgeFxParamLayout {
        fx_type,
        display_name,
        posy_offset: [0; super::SURGE_FX_PARAM_COUNT],
        active: [false; super::SURGE_FX_PARAM_COUNT],
        names: std::array::from_fn(|_| String::new()),
    };
    // Read the constructor default rather than assuming the placement offset.
    let cpp = Cpp::parse(parameter)?;
    let body = cpp.function("Parameter::Parameter")?;
    let assignment = nodes(body)
        .into_iter()
        .find(|n| {
            n.kind() == "assignment_expression"
                && field(*n, "left").is_ok_and(|n| text(n, parameter) == "posy_offset")
        })
        .context("Parameter constructor has no posy_offset initializer")?;
    let default = env
        .eval(field(assignment, "right")?, parameter)?
        .integer()?;
    result.posy_offset.fill(i32::try_from(default)?);
    controls::run(
        implementation,
        "::init_ctrltypes",
        &mut env,
        &mut result,
        Some(effect_base),
    )?;
    if let Some(source) = sst_header {
        let cpp = Cpp::parse(source)?;
        let body = cpp.function("paramAt")?;
        for case in nodes(body)
            .into_iter()
            .filter(|n| n.kind() == "case_statement")
        {
            let Some(value) = case.child_by_field_name("value") else {
                continue;
            };
            let index = usize::try_from(env.eval(value, source)?.integer()?)?;
            let names: Vec<_> = nodes(case)
                .into_iter()
                .filter(|n| {
                    n.kind() == "call_expression"
                        && field(*n, "function")
                            .is_ok_and(|f| text(f, source).ends_with(".withName"))
                })
                .collect();
            ensure!(names.len() <= 1, "ambiguous sst parameter name");
            if let Some(call) = names.first() {
                let args = children(field(*call, "arguments")?);
                ensure!(args.len() == 1, "withName arity changed");
                *result
                    .names
                    .get_mut(index)
                    .context("sst parameter index out of range")? =
                    env.eval(args[0], source)?.string()?;
            }
        }
    }
    ensure!(
        result.active.iter().any(|v| *v),
        "effect has no active parameters"
    );
    // Some controls have only a dynamicName callback. An empty static name is
    // intentional and does not affect the storage/state index mapping.
    Ok(result)
}

#[cfg(test)]
mod tests;
