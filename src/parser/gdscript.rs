//! GDScript AST support. Unknown receiver types are not guessed as local calls.
use super::{node_text, relations::ParsedRelation, treesitter::ParsedNode};
use crate::domain::{max_code_content_len, MAX_AST_DEPTH};
use tree_sitter::{Node, Tree};

fn field(node: Node<'_>, key: &str, source: &str) -> Option<String> {
    node.child_by_field_name(key)
        .map(|n| node_text(&n, source).to_string())
}

fn script_class(root: Node<'_>, source: &str) -> Option<String> {
    root.named_children(&mut root.walk())
        .find(|n| n.kind() == "class_name_statement")
        .and_then(|n| field(n, "name", source))
}

pub(super) fn nodes(tree: &Tree, source: &str) -> Vec<ParsedNode> {
    let mut out = Vec::new();
    let class = script_class(tree.root_node(), source);
    walk_nodes(
        tree.root_node(),
        source,
        class.as_deref(),
        false,
        0,
        &mut out,
    );
    out
}

fn walk_nodes(
    node: Node<'_>,
    source: &str,
    class: Option<&str>,
    in_function: bool,
    depth: usize,
    out: &mut Vec<ParsedNode>,
) {
    if depth >= MAX_AST_DEPTH {
        return;
    }
    let kind = match node.kind() {
        "function_definition" => Some(if class.is_some() {
            "method"
        } else {
            "function"
        }),
        "class_definition" | "class_name_statement" => Some("class"),
        "signal_statement" => Some("variable"),
        "enum_definition" => Some("enum"),
        "const_statement" if !in_function => Some("constant"),
        "variable_statement" if !in_function => Some("variable"),
        _ => None,
    };
    let name = field(node, "name", source);
    if let (Some(kind), Some(name)) = (kind, name.as_ref()) {
        let code = node_text(&node, source);
        let mut end = code.len().min(max_code_content_len());
        while !code.is_char_boundary(end) {
            end -= 1;
        }
        let mut docs = Vec::new();
        let mut prev = node.prev_named_sibling();
        while let Some(p) = prev {
            if p.kind() != "comment" || !node_text(&p, source).starts_with("##") {
                break;
            }
            docs.push(
                node_text(&p, source)
                    .trim_start_matches('#')
                    .trim()
                    .to_string(),
            );
            prev = p.prev_named_sibling();
        }
        docs.reverse();
        out.push(ParsedNode {
            node_type: kind.to_string(),
            name: name.clone(),
            qualified_name: Some(if kind != "class" {
                class
                    .map(|c| format!("{c}.{name}"))
                    .unwrap_or_else(|| name.clone())
            } else {
                name.clone()
            }),
            start_line: node.start_position().row as u32 + 1,
            end_line: node.end_position().row as u32 + 1,
            code_content: code[..end].to_string(),
            signature: Some(code.lines().next().unwrap_or_default().to_string()),
            doc_comment: (!docs.is_empty()).then(|| docs.join("\n")),
            return_type: field(node, "return_type", source),
            param_types: field(node, "parameters", source),
            is_test: false,
        });
    }
    let nested_class = if node.kind() == "class_definition" {
        name.as_deref()
    } else {
        class
    };
    let in_function = in_function || matches!(node.kind(), "function_definition" | "lambda");
    for child in node.named_children(&mut node.walk()) {
        walk_nodes(child, source, nested_class, in_function, depth + 1, out);
    }
}

pub(super) fn relations(tree: &Tree, source: &str) -> Vec<ParsedRelation> {
    let mut out = Vec::new();
    let class = script_class(tree.root_node(), source);
    walk_relations(
        tree.root_node(),
        source,
        class.as_deref(),
        None,
        0,
        &mut out,
    );
    out
}

fn walk_relations(
    node: Node<'_>,
    source: &str,
    class: Option<&str>,
    scope: Option<(&str, u32)>,
    depth: usize,
    out: &mut Vec<ParsedRelation>,
) {
    if depth >= MAX_AST_DEPTH {
        return;
    }
    let name = field(node, "name", source);
    let class = if node.kind() == "class_definition" {
        name.as_deref()
    } else {
        class
    };
    let function_name = if node.kind() == "function_definition" {
        name.as_ref().map(|n| {
            class
                .map(|c| format!("{c}.{n}"))
                .unwrap_or_else(|| n.clone())
        })
    } else {
        None
    };
    let scope = function_name
        .as_deref()
        .map(|n| (n, node.start_position().row as u32 + 1))
        .or(scope);
    if node.kind() == "call" {
        // Bare calls only: engine/other receiver calls must not bind to a same-name method.
        if let Some(callee) = node.named_child(0).filter(|n| n.kind() == "identifier") {
            if let Some((caller, line)) = scope {
                out.push(ParsedRelation {
                    source_name: caller.to_string(),
                    target_name: node_text(&callee, source).to_string(),
                    relation: "calls".to_string(),
                    metadata: None,
                    source_language: "gdscript".to_string(),
                    source_line: Some(line),
                });
            }
        }
    }
    if node.kind() == "attribute"
        && node.named_child_count() == 2
        && node
            .named_child(0)
            .is_some_and(|n| node_text(&n, source) == "self")
    {
        if let (Some((caller, line)), Some(call)) = (
            scope,
            node.named_child(1).filter(|n| n.kind() == "attribute_call"),
        ) {
            if let Some(callee) = call.named_child(0).filter(|n| n.kind() == "identifier") {
                out.push(ParsedRelation {
                    source_name: caller.to_string(),
                    target_name: node_text(&callee, source).to_string(),
                    relation: "calls".to_string(),
                    metadata: None,
                    source_language: "gdscript".to_string(),
                    source_line: Some(line),
                });
            }
        }
    }
    for child in node.named_children(&mut node.walk()) {
        walk_relations(child, source, class, scope, depth + 1, out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::treesitter::parse_tree;

    #[test]
    fn gdscript_bootstrap_fixture() {
        let source = include_str!("../../tests/fixtures/gdscript/bootstrap.gd");
        let tree = parse_tree(source, "gdscript").unwrap();
        assert!(!tree.root_node().has_error());
        let found = nodes(&tree, source);
        for name in ["_ready", "_on_quit_pressed"] {
            let n = found.iter().find(|n| n.name == name).unwrap();
            assert_eq!(n.return_type.as_deref(), Some("void"));
            assert!(n.code_content.contains("get_tree()"));
            assert!(n.start_line > 0);
        }
    }

    #[test]
    fn gdscript_scopes_and_calls() {
        let source = "class_name Example\nextends Node\nsignal changed(value: int)\n@export var count: int = 0\nconst LIMIT = 3\nfunc run(value: int) -> void:\n\t# fake_call()\n\tvar message = \"fake_call()\"\n\thelper(value)\n\tself.helper(value)\n\tother.helper(value)\nfunc helper(value: int) -> void:\n\tpass\n";
        let tree = parse_tree(source, "gdscript").unwrap();
        assert!(
            !tree.root_node().has_error(),
            "{}",
            tree.root_node().to_sexp()
        );
        let found = nodes(&tree, source);
        assert_eq!(found.len(), 6);
        assert!(found
            .iter()
            .any(|n| n.qualified_name.as_deref() == Some("Example.run")));
        assert!(!found.iter().any(|n| n.name == "message"));
        let calls = relations(&tree, source);
        assert_eq!(calls.len(), 2);
        assert!(calls
            .iter()
            .all(|r| r.source_name == "Example.run" && r.target_name == "helper"));
    }
}
