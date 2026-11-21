use anyhow::{bail, Context, Result};
use tree_sitter::{Node, Parser, Tree};

pub struct Cpp {
    tree: Tree,
    source: String,
}

impl Cpp {
    pub fn parse(source: &str) -> Result<Self> {
        let mut parser = Parser::new();
        parser.set_language(&tree_sitter_cpp::LANGUAGE.into())?;
        let tree = parser
            .parse(source, None)
            .context("C++ parsing interrupted")?;
        Ok(Self {
            tree,
            source: source.to_owned(),
        })
    }

    pub fn root(&self) -> Node<'_> {
        self.tree.root_node()
    }

    pub fn function(&self, suffix: &str) -> Result<Node<'_>> {
        let matches: Vec<_> = nodes(self.root())
            .into_iter()
            .filter(|n| {
                n.kind() == "function_definition"
                    && n.child_by_field_name("declarator").is_some_and(|d| {
                        let name = text(d, &self.source).split('(').next().unwrap_or("").trim();
                        name.ends_with(suffix)
                    })
            })
            .collect();
        if matches.len() != 1 {
            bail!(
                "expected one C++ function {suffix}, found {}",
                matches.len()
            )
        }
        let body = field(matches[0], "body")?;
        if body.has_error() {
            bail!("invalid C++ body for {suffix}")
        }
        Ok(body)
    }
}

pub fn text<'s>(node: Node<'_>, source: &'s str) -> &'s str {
    &source[node.byte_range()]
}

pub fn field<'a>(node: Node<'a>, name: &str) -> Result<Node<'a>> {
    node.child_by_field_name(name)
        .with_context(|| format!("missing C++ {name} in {}", node.kind()))
}

pub fn children(node: Node<'_>) -> Vec<Node<'_>> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor)
        .filter(|n| n.kind() != "comment")
        .collect()
}

pub fn nodes(node: Node<'_>) -> Vec<Node<'_>> {
    let mut out = vec![node];
    for child in children(node) {
        out.extend(nodes(child));
    }
    out
}
