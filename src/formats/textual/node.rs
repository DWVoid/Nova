pub(super) struct Formatter {
    pub(super) out: String,
    indent: usize,
    newline: bool,
}

impl Formatter {
    pub(super) fn new() -> Self {
        Self {
            out: String::new(),
            indent: 0,
            newline: true,
        }
    }

    pub(super) fn write(&mut self, text: &str) {
        if self.newline {
            for _ in 0..self.indent {
                self.out.push(' ');
            }
            self.newline = false;
        }
        self.out.push_str(text);
    }

    pub(super) fn write_line(&mut self, text: &str) {
        if self.newline {
            for _ in 0..self.indent {
                self.out.push(' ');
            }
        }
        self.out.push_str(text);
        self.out.push('\n');
        self.newline = true;
    }

    pub(super) fn indent(&mut self) {
        self.indent += 2;
    }

    pub(super) fn unindent(&mut self) {
        self.indent -= 2;
    }
}

pub(super) trait Node {
    fn format(&self, out: &mut Formatter);
}

pub(super) struct BoolNode {
    pub(super) bool: bool,
}

pub(super) struct NumericNode {
    pub(super) int: String, // Use String to represent numerics of any size
}

pub(super) struct StringNode {
    pub(super) string: String,
}

pub(super) struct ArrayNode {
    pub(super) elements: Vec<TextualNode>,
}

pub(super) struct ObjectNode {
    pub(super) tag: String,
    pub(super) fields: Vec<(String, TextualNode)>,
}

pub(super) enum TextualNode {
    Bool(BoolNode),
    Numeric(NumericNode),
    String(StringNode),
    Array(ArrayNode),
    Object(ObjectNode),
}

impl Node for BoolNode {
    fn format(&self, out: &mut Formatter) {
        out.write(if self.bool { "true" } else { "false" });
    }
}

impl Node for NumericNode {
    fn format(&self, out: &mut Formatter) {
        out.write(&self.int);
    }
}

impl Node for StringNode {
    fn format(&self, out: &mut Formatter) {
        // Produce a quoted string with escape sequences for special characters.
        let mut s = String::with_capacity(self.string.len() + 2);
        s.push('"');
        for ch in self.string.chars() {
            match ch {
                '"'  => s.push_str("\\\""),
                '\\' => s.push_str("\\\\"),
                '\n' => s.push_str("\\n"),
                '\r' => s.push_str("\\r"),
                '\t' => s.push_str("\\t"),
                c    => s.push(c),
            }
        }
        s.push('"');
        out.write(&s);
    }
}

impl Node for ArrayNode {
    fn format(&self, out: &mut Formatter) {
        if self.elements.is_empty() {
            out.write("[]");
            return;
        }
        out.write_line("[");
        out.indent();
        for (i, elem) in self.elements.iter().enumerate() {
            elem.format(out);
            // Write a trailing comma on every element except the last.
            if i < self.elements.len() - 1 {
                out.write_line(",");
            } else {
                out.write_line("");
            }
        }
        out.unindent();
        out.write("]");
    }
}

impl Node for ObjectNode {
    fn format(&self, out: &mut Formatter) {
        // Always write the tag name.
        if self.fields.is_empty() {
            // Unit-like object: just the tag.
            out.write(&self.tag);
            return;
        }
        out.write_line(&format!("{} {{", self.tag));
        out.indent();
        for (i, (key, value)) in self.fields.iter().enumerate() {
            out.write(&format!("{}: ", key));
            value.format(out);
            if i < self.fields.len() - 1 {
                out.write_line(",");
            } else {
                out.write_line("");
            }
        }
        out.unindent();
        out.write("}");
    }
}

impl Node for TextualNode {
    fn format(&self, out: &mut Formatter) {
        match self {
            TextualNode::Bool(n)    => n.format(out),
            TextualNode::Numeric(n) => n.format(out),
            TextualNode::String(n)  => n.format(out),
            TextualNode::Array(n)   => n.format(out),
            TextualNode::Object(n)  => n.format(out),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Convenience: format a node into a fresh Formatter and return the output string.
    fn fmt(node: &dyn Node) -> String {
        let mut f = Formatter::new();
        node.format(&mut f);
        f.out
    }

    // ── BoolNode ────────────────────────────────────────────────────────────

    #[test]
    fn bool_true() {
        assert_eq!(fmt(&BoolNode { bool: true }), "true");
    }

    #[test]
    fn bool_false() {
        assert_eq!(fmt(&BoolNode { bool: false }), "false");
    }

    // ── NumericNode ─────────────────────────────────────────────────────────

    #[test]
    fn numeric_integer() {
        assert_eq!(fmt(&NumericNode { int: "42".to_string() }), "42");
    }

    #[test]
    fn numeric_negative() {
        assert_eq!(fmt(&NumericNode { int: "-7".to_string() }), "-7");
    }

    #[test]
    fn numeric_float() {
        assert_eq!(fmt(&NumericNode { int: "3.14".to_string() }), "3.14");
    }

    #[test]
    fn numeric_large() {
        let big = "99999999999999999999999999999999";
        assert_eq!(fmt(&NumericNode { int: big.to_string() }), big);
    }

    // ── StringNode ──────────────────────────────────────────────────────────

    #[test]
    fn string_plain() {
        assert_eq!(fmt(&StringNode { string: "hello".to_string() }), r#""hello""#);
    }

    #[test]
    fn string_empty() {
        assert_eq!(fmt(&StringNode { string: String::new() }), r#""""#);
    }

    #[test]
    fn string_with_double_quote() {
        assert_eq!(fmt(&StringNode { string: r#"say "hi""#.to_string() }), r#""say \"hi\"""#);
    }

    #[test]
    fn string_with_backslash() {
        assert_eq!(fmt(&StringNode { string: r"a\b".to_string() }), r#""a\\b""#);
    }

    #[test]
    fn string_escape_sequences() {
        assert_eq!(
            fmt(&StringNode { string: "a\nb\rc\td".to_string() }),
            r#""a\nb\rc\td""#
        );
    }

    // ── ArrayNode ───────────────────────────────────────────────────────────

    #[test]
    fn array_empty() {
        assert_eq!(fmt(&ArrayNode { elements: vec![] }), "[]");
    }

    #[test]
    fn array_single_element() {
        let node = ArrayNode {
            elements: vec![TextualNode::Numeric(NumericNode { int: "1".to_string() })],
        };
        let expected = "[\n  1\n]";
        assert_eq!(fmt(&node), expected);
    }

    #[test]
    fn array_multiple_elements() {
        let node = ArrayNode {
            elements: vec![
                TextualNode::Numeric(NumericNode { int: "1".to_string() }),
                TextualNode::Numeric(NumericNode { int: "2".to_string() }),
                TextualNode::Numeric(NumericNode { int: "3".to_string() }),
            ],
        };
        let expected = "[\n  1,\n  2,\n  3\n]";
        assert_eq!(fmt(&node), expected);
    }

    #[test]
    fn array_of_strings() {
        let node = ArrayNode {
            elements: vec![
                TextualNode::String(StringNode { string: "a".to_string() }),
                TextualNode::String(StringNode { string: "b".to_string() }),
            ],
        };
        let expected = "[\n  \"a\",\n  \"b\"\n]";
        assert_eq!(fmt(&node), expected);
    }

    #[test]
    fn array_of_bools() {
        let node = ArrayNode {
            elements: vec![
                TextualNode::Bool(BoolNode { bool: true }),
                TextualNode::Bool(BoolNode { bool: false }),
            ],
        };
        let expected = "[\n  true,\n  false\n]";
        assert_eq!(fmt(&node), expected);
    }

    // ── ObjectNode ──────────────────────────────────────────────────────────

    #[test]
    fn object_unit_like() {
        let node = ObjectNode { tag: "None".to_string(), fields: vec![] };
        assert_eq!(fmt(&node), "None");
    }

    #[test]
    fn object_single_field() {
        let node = ObjectNode {
            tag: "Point".to_string(),
            fields: vec![
                ("x".to_string(), TextualNode::Numeric(NumericNode { int: "10".to_string() })),
            ],
        };
        let expected = "Point {\n  x: 10\n}";
        assert_eq!(fmt(&node), expected);
    }

    #[test]
    fn object_multiple_fields() {
        let node = ObjectNode {
            tag: "Point".to_string(),
            fields: vec![
                ("x".to_string(), TextualNode::Numeric(NumericNode { int: "10".to_string() })),
                ("y".to_string(), TextualNode::Numeric(NumericNode { int: "20".to_string() })),
            ],
        };
        let expected = "Point {\n  x: 10,\n  y: 20\n}";
        assert_eq!(fmt(&node), expected);
    }

    #[test]
    fn object_field_with_string_value() {
        let node = ObjectNode {
            tag: "Person".to_string(),
            fields: vec![
                ("name".to_string(), TextualNode::String(StringNode { string: "Alice".to_string() })),
                ("age".to_string(),  TextualNode::Numeric(NumericNode { int: "30".to_string() })),
            ],
        };
        let expected = "Person {\n  name: \"Alice\",\n  age: 30\n}";
        assert_eq!(fmt(&node), expected);
    }

    // ── TextualNode enum dispatch ────────────────────────────────────────────

    #[test]
    fn textual_node_bool() {
        assert_eq!(fmt(&TextualNode::Bool(BoolNode { bool: true })), "true");
    }

    #[test]
    fn textual_node_numeric() {
        assert_eq!(fmt(&TextualNode::Numeric(NumericNode { int: "0".to_string() })), "0");
    }

    #[test]
    fn textual_node_string() {
        assert_eq!(fmt(&TextualNode::String(StringNode { string: "hi".to_string() })), "\"hi\"");
    }

    #[test]
    fn textual_node_array() {
        assert_eq!(fmt(&TextualNode::Array(ArrayNode { elements: vec![] })), "[]");
    }

    #[test]
    fn textual_node_object_unit() {
        assert_eq!(
            fmt(&TextualNode::Object(ObjectNode { tag: "Unit".to_string(), fields: vec![] })),
            "Unit"
        );
    }

    // ── Nested structures ────────────────────────────────────────────────────

    #[test]
    fn nested_array_in_object() {
        let node = ObjectNode {
            tag: "Wrapper".to_string(),
            fields: vec![(
                "items".to_string(),
                TextualNode::Array(ArrayNode {
                    elements: vec![
                        TextualNode::Numeric(NumericNode { int: "1".to_string() }),
                        TextualNode::Numeric(NumericNode { int: "2".to_string() }),
                    ],
                }),
            )],
        };
        let expected = "Wrapper {\n  items: [\n    1,\n    2\n  ]\n}";
        assert_eq!(fmt(&node), expected);
    }

    #[test]
    fn nested_object_in_array() {
        let node = ArrayNode {
            elements: vec![
                TextualNode::Object(ObjectNode {
                    tag: "A".to_string(),
                    fields: vec![
                        ("v".to_string(), TextualNode::Bool(BoolNode { bool: true })),
                    ],
                }),
                TextualNode::Object(ObjectNode {
                    tag: "B".to_string(),
                    fields: vec![],
                }),
            ],
        };
        let expected = "[\n  A {\n    v: true\n  },\n  B\n]";
        assert_eq!(fmt(&node), expected);
    }

    #[test]
    fn deeply_nested_objects() {
        let inner = TextualNode::Object(ObjectNode {
            tag: "Inner".to_string(),
            fields: vec![
                ("val".to_string(), TextualNode::Numeric(NumericNode { int: "99".to_string() })),
            ],
        });
        let outer = ObjectNode {
            tag: "Outer".to_string(),
            fields: vec![("child".to_string(), inner)],
        };
        let expected = "Outer {\n  child: Inner {\n    val: 99\n  }\n}";
        assert_eq!(fmt(&outer), expected);
    }
}
