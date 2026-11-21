use super::expression::{declaration_name, Environment, Value};
use super::syntax::{children, field, text, Cpp};
use crate::surge_fx::SurgeFxParamLayout;
use anyhow::{bail, ensure, Context, Result};
use tree_sitter::Node;

pub fn run(
    source: &str,
    function: &str,
    env: &mut Environment,
    layout: &mut SurgeFxParamLayout,
    effect_base: Option<&str>,
) -> Result<()> {
    let cpp = Cpp::parse(source)?;
    Executor {
        source,
        env,
        layout,
        steps: 0,
        effect_base,
    }
    .statement(cpp.function(function)?)
}

struct Executor<'a> {
    source: &'a str,
    env: &'a mut Environment,
    layout: &'a mut SurgeFxParamLayout,
    steps: usize,
    effect_base: Option<&'a str>,
}

impl Executor<'_> {
    fn statement(&mut self, node: Node<'_>) -> Result<()> {
        self.steps += 1;
        ensure!(
            self.steps < 10000,
            "C++ extraction iteration limit exceeded"
        );
        let raw = text(node, self.source);
        match node.kind() {
            "compound_statement" => {
                for child in children(node) {
                    self.statement(child)?;
                }
            }
            "declaration" => {
                // Struct methods define GUI callbacks; they are not executed by init_ctrltypes.
                if children(node)
                    .iter()
                    .any(|n| matches!(n.kind(), "struct_specifier" | "class_specifier"))
                {
                    return Ok(());
                }
                for decl in children(node)
                    .into_iter()
                    .filter(|n| n.kind() == "init_declarator")
                {
                    let name = declaration_name(field(decl, "declarator")?, self.source)
                        .context("unsupported local declaration")?;
                    let value = self.env.eval(field(decl, "value")?, self.source)?;
                    self.env.0.insert(name, value);
                }
            }
            "for_statement" => {
                self.statement(field(node, "initializer")?)?;
                loop {
                    if self
                        .env
                        .eval(field(node, "condition")?, self.source)?
                        .integer()?
                        == 0
                    {
                        break;
                    }
                    self.statement(field(node, "body")?)?;
                    self.statement(field(node, "update")?)?;
                }
            }
            "if_statement" => {
                // GUI-only formatter allocation does not contribute to a layout.
                if !self.relevant(node) {
                    return Ok(());
                }
                if self
                    .env
                    .eval(field(node, "condition")?, self.source)?
                    .integer()?
                    != 0
                {
                    self.statement(field(node, "consequence")?)?;
                } else if let Some(other) = node.child_by_field_name("alternative") {
                    self.statement(other)?;
                }
            }
            "else_clause" | "expression_statement" => {
                for child in children(node) {
                    self.statement(child)?;
                }
            }
            "update_expression" => {
                let name = text(field(node, "argument")?, self.source);
                let value = self.env.integer_name(name)?;
                let op = text(field(node, "operator")?, self.source);
                let value = match op {
                    "++" => value.checked_add(1),
                    "--" => value.checked_sub(1),
                    _ => None,
                }
                .context("unsupported update or integer overflow")?;
                self.env.0.insert(name.into(), Value::Integer(value));
            }
            "assignment_expression" => self.assign(node)?,
            "call_expression" => self.call(node)?,
            "comment" | ";" => {}
            _ if !self.relevant(node) => {}
            _ => bail!("unsupported layout statement {}: {raw}", node.kind()),
        }
        Ok(())
    }

    fn relevant(&self, node: Node<'_>) -> bool {
        let raw = text(node, self.source);
        raw.contains("posy_offset")
            || raw.contains("set_type")
            || raw.contains("set_name")
            || super::syntax::nodes(node).iter().any(|n| {
                (n.kind() == "call_expression"
                    && n.child_by_field_name("function")
                        .is_some_and(|f| !text(f, self.source).starts_with("std::make_unique<")))
                    || n.kind() == "assignment_expression"
                        && n.child_by_field_name("left").is_some_and(|left| {
                            left.kind() == "identifier"
                                && self.env.0.contains_key(text(left, self.source))
                        })
            })
    }

    fn assign(&mut self, node: Node<'_>) -> Result<()> {
        let left = field(node, "left")?;
        let op = text(field(node, "operator")?, self.source);
        if left.kind() == "field_expression" {
            let member = text(field(left, "field")?, self.source);
            if member != "posy_offset" {
                return Ok(());
            }
            let index = self.parameter_index(field(left, "argument")?)?;
            let value = i32::try_from(
                self.env
                    .eval(field(node, "right")?, self.source)?
                    .integer()?,
            )?;
            ensure!(op == "=", "unsupported posy_offset assignment");
            self.layout.posy_offset[index] = value;
        } else if left.kind() == "identifier" && self.env.0.contains_key(text(left, self.source)) {
            let name = text(left, self.source).to_owned();
            let value = self.env.eval(field(node, "right")?, self.source)?;
            let value = match op {
                "=" => value,
                "+=" => Value::Integer(
                    self.env
                        .integer_name(&name)?
                        .checked_add(value.integer()?)
                        .context("integer overflow")?,
                ),
                _ => bail!("unsupported local assignment {op}"),
            };
            self.env.0.insert(name, value);
        } else {
            ensure!(
                !text(left, self.source).contains("posy_offset"),
                "unsupported placement target"
            );
        }
        Ok(())
    }

    fn call(&mut self, node: Node<'_>) -> Result<()> {
        let function = field(node, "function")?;
        let raw = text(function, self.source);
        if function.kind() != "field_expression" {
            ensure!(
                raw == "Effect::init_ctrltypes" || raw == "configureControlsFromFXMetadata",
                "unsupported init helper {raw}"
            );
            if raw == "Effect::init_ctrltypes" {
                let source = self
                    .effect_base
                    .context("recursive Effect::init_ctrltypes")?;
                run(
                    source,
                    "Effect::init_ctrltypes",
                    self.env,
                    self.layout,
                    None,
                )?;
            }
            return Ok(());
        }
        let member = text(field(function, "field")?, self.source);
        if !matches!(member, "set_type" | "set_name") {
            ensure!(
                member == "set_user_data",
                "unsupported parameter method {member}"
            );
            return Ok(());
        }
        let index = self.parameter_index(field(function, "argument")?)?;
        let args = children(field(node, "arguments")?);
        ensure!(args.len() == 1, "parameter method arity changed");
        if member == "set_name" {
            self.layout.names[index] = self.env.eval(args[0], self.source)?.string()?;
        } else {
            let ctrl = text(args[0], self.source);
            ensure!(
                args[0].kind() == "identifier" && ctrl.starts_with("ct_"),
                "unsupported control type {ctrl}"
            );
            self.layout.active[index] = ctrl != "ct_none";
        }
        Ok(())
    }

    fn parameter_index(&self, node: Node<'_>) -> Result<usize> {
        ensure!(
            node.kind() == "subscript_expression",
            "unsupported parameter target"
        );
        let target = field(node, "argument")?;
        ensure!(
            text(target, self.source) == "fxdata->p",
            "unexpected parameter array"
        );
        let index = usize::try_from(
            self.env
                .eval(field(node, "indices")?, self.source)?
                .integer()?,
        )?;
        ensure!(
            index < self.layout.active.len(),
            "parameter index out of range: {index}"
        );
        Ok(index)
    }
}
