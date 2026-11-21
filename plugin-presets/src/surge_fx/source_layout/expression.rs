use super::syntax::{children, field, nodes, text, Cpp};
use anyhow::{bail, ensure, Context, Result};
use std::collections::HashMap;
use tree_sitter::Node;

#[derive(Clone, Debug)]
pub enum Value {
    Integer(i64),
    String(String),
    Array(Vec<Value>),
}

impl Value {
    pub fn integer(&self) -> Result<i64> {
        match self {
            Self::Integer(v) => Ok(*v),
            _ => bail!("expected integer, got {self:?}"),
        }
    }
    pub fn string(&self) -> Result<String> {
        match self {
            Self::String(v) => Ok(v.clone()),
            _ => bail!("expected string, got {self:?}"),
        }
    }
}

#[derive(Clone, Default)]
pub struct Environment(pub HashMap<String, Value>);

impl Environment {
    pub fn integer_name(&self, name: &str) -> Result<i64> {
        self.0
            .get(name)
            .with_context(|| format!("unknown C++ constant {name}"))?
            .integer()
    }

    /// Collect only evaluable constant declarations. Unsupported declarations
    /// remain unresolved and are errors if the extraction actually uses them.
    pub fn collect(&mut self, source: &str) -> Result<()> {
        let cpp = Cpp::parse(source)?;
        for node in nodes(cpp.root()) {
            if node.kind() == "enumerator_list" {
                let mut next = Some(0_i64);
                for member in children(node) {
                    let value = if let Some(v) = member.child_by_field_name("value") {
                        self.eval(v, source).and_then(|v| v.integer()).ok()
                    } else {
                        next
                    };
                    if let Some(value) = value {
                        let name = text(field(member, "name")?, source).to_owned();
                        self.0.insert(name, Value::Integer(value));
                    }
                    next = value.and_then(|v| v.checked_add(1));
                }
            } else if matches!(node.kind(), "init_declarator" | "field_declaration") {
                let Some(value) = node
                    .child_by_field_name("value")
                    .or_else(|| node.child_by_field_name("default_value"))
                else {
                    continue;
                };
                let Some(decl) = node.child_by_field_name("declarator") else {
                    continue;
                };
                if let (Some(name), Ok(value)) =
                    (declaration_name(decl, source), self.eval(value, source))
                {
                    self.0.insert(name, value);
                }
            }
        }
        Ok(())
    }

    pub fn eval(&self, node: Node<'_>, source: &str) -> Result<Value> {
        let raw = text(node, source);
        match node.kind() {
            "number_literal" => Ok(Value::Integer(
                raw.parse()
                    .with_context(|| format!("unsupported integer {raw}"))?,
            )),
            "true" => Ok(Value::Integer(1)),
            "false" => Ok(Value::Integer(0)),
            "identifier" | "field_identifier" | "qualified_identifier" => self
                .0
                .get(raw)
                .cloned()
                .with_context(|| format!("unresolved C++ value {raw}")),
            "string_literal" => Ok(Value::String(serde_json::from_str(raw)?)),
            "initializer_list" => Ok(Value::Array(
                children(node)
                    .into_iter()
                    .map(|n| self.eval(n, source))
                    .collect::<Result<_>>()?,
            )),
            "parenthesized_expression" | "condition_clause" | "argument_list" => {
                let c = children(node);
                ensure!(c.len() == 1, "expected one expression: {raw}");
                self.eval(c[0], source)
            }
            "unary_expression" => {
                let value = self.eval(field(node, "argument")?, source)?.integer()?;
                let operator = text(field(node, "operator")?, source);
                let result = match operator {
                    "-" => value.checked_neg().context("integer overflow")?,
                    "+" => value,
                    "!" => i64::from(value == 0),
                    "~" => !value,
                    _ => bail!("unsupported unary operator {operator}"),
                };
                Ok(Value::Integer(result))
            }
            "binary_expression" => {
                let left = self.eval(field(node, "left")?, source)?;
                let right = self.eval(field(node, "right")?, source)?;
                let op = text(field(node, "operator")?, source);
                if let (Value::String(l), Value::String(r)) = (&left, &right) {
                    ensure!(op == "+", "unsupported string operator {op}");
                    return Ok(Value::String(format!("{l}{r}")));
                }
                let (l, r) = (left.integer()?, right.integer()?);
                let checked = match op {
                    "+" => l.checked_add(r),
                    "-" => l.checked_sub(r),
                    "*" => l.checked_mul(r),
                    "/" => l.checked_div(r),
                    "%" => l.checked_rem(r),
                    "==" => Some(i64::from(l == r)),
                    "!=" => Some(i64::from(l != r)),
                    "<" => Some(i64::from(l < r)),
                    "<=" => Some(i64::from(l <= r)),
                    ">" => Some(i64::from(l > r)),
                    ">=" => Some(i64::from(l >= r)),
                    "&&" => Some(i64::from(l != 0 && r != 0)),
                    "||" => Some(i64::from(l != 0 || r != 0)),
                    "|" => Some(l | r),
                    "&" => Some(l & r),
                    _ => bail!("unsupported integer operator {op}"),
                }
                .context("invalid integer arithmetic")?;
                Ok(Value::Integer(checked))
            }
            "subscript_expression" => {
                let array = self.eval(field(node, "argument")?, source)?;
                let index =
                    usize::try_from(self.eval(field(node, "indices")?, source)?.integer()?)?;
                let Value::Array(values) = array else {
                    bail!("not an array: {raw}")
                };
                values
                    .get(index)
                    .cloned()
                    .context("array index out of range")
            }
            "subscript_argument_list" => {
                let c = children(node);
                ensure!(c.len() == 1, "unsupported array indices");
                self.eval(c[0], source)
            }
            "call_expression" => {
                let function = field(node, "function")?;
                let args = children(field(node, "arguments")?);
                if text(function, source) == "std::to_string" {
                    ensure!(args.len() == 1, "to_string arity changed");
                    return Ok(Value::String(
                        self.eval(args[0], source)?.integer()?.to_string(),
                    ));
                }
                if function.kind() == "field_expression"
                    && text(field(function, "field")?, source) == "c_str"
                {
                    ensure!(args.is_empty(), "c_str arity changed");
                    return self.eval(field(function, "argument")?, source);
                }
                bail!("unsupported value-producing call {raw}")
            }
            _ => bail!("unsupported C++ expression {}: {raw}", node.kind()),
        }
    }
}

pub fn declaration_name(node: Node<'_>, source: &str) -> Option<String> {
    match node.kind() {
        "identifier" | "field_identifier" => Some(text(node, source).to_owned()),
        "array_declarator" | "pointer_declarator" | "reference_declarator" => node
            .child_by_field_name("declarator")
            .and_then(|n| declaration_name(n, source)),
        _ => None,
    }
}
