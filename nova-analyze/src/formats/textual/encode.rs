use super::node::*;
use serde::ser;
use std::fmt;

// ── Error ────────────────────────────────────────────────────────────────────

#[derive(Debug)]
pub struct Error(pub(super) String);

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Encoder error: {}", self.0)
    }
}

impl std::error::Error for Error {}

impl ser::Error for Error {
    fn custom<T: fmt::Display>(msg: T) -> Self { Error(msg.to_string()) }
}

// ── Compound encoder structs ─────────────────────────────────────────────────

/// Accumulates elements for a sequence or tuple into an `ArrayNode`.
pub(super) struct SeqEncoder {
    elements: Vec<TextualNode>,
}

/// Accumulates fields for a tuple variant into an `ObjectNode`.
/// Fields have no names, so keys are left empty.
pub(super) struct TupleVariantEncoder {
    tag: String,
    fields: Vec<(String, TextualNode)>,
}

/// Accumulates key-value pairs for a map into an `ObjectNode` tagged "map".
pub(super) struct MapEncoder {
    fields: Vec<(String, TextualNode)>,
    pending_key: Option<String>,
}

/// Accumulates named fields for a struct into an `ObjectNode`.
pub(super) struct StructEncoder {
    tag: String,
    fields: Vec<(String, TextualNode)>,
}

/// Accumulates named fields for a struct variant into an `ObjectNode`.
pub(super) struct StructVariantEncoder {
    tag: String,
    fields: Vec<(String, TextualNode)>,
}

// ── SerializeSeq / SerializeTuple / SerializeTupleStruct ────────────────────

impl ser::SerializeSeq for SeqEncoder {
    type Ok = TextualNode;
    type Error = Error;

    fn serialize_element<T: ?Sized + serde::Serialize>(&mut self, value: &T) -> Result<(), Error> {
        self.elements.push(value.serialize(Encoder)?);
        Ok(())
    }

    fn end(self) -> Result<TextualNode, Error> {
        Ok(TextualNode::Array(ArrayNode { elements: self.elements }))
    }
}

impl ser::SerializeTuple for SeqEncoder {
    type Ok = TextualNode;
    type Error = Error;

    fn serialize_element<T: ?Sized + serde::Serialize>(&mut self, value: &T) -> Result<(), Error> {
        ser::SerializeSeq::serialize_element(self, value)
    }

    fn end(self) -> Result<TextualNode, Error> {
        ser::SerializeSeq::end(self)
    }
}

impl ser::SerializeTupleStruct for SeqEncoder {
    type Ok = TextualNode;
    type Error = Error;

    fn serialize_field<T: ?Sized + serde::Serialize>(&mut self, value: &T) -> Result<(), Error> {
        ser::SerializeSeq::serialize_element(self, value)
    }

    fn end(self) -> Result<TextualNode, Error> {
        ser::SerializeSeq::end(self)
    }
}

// ── SerializeTupleVariant ────────────────────────────────────────────────────

impl ser::SerializeTupleVariant for TupleVariantEncoder {
    type Ok = TextualNode;
    type Error = Error;

    fn serialize_field<T: ?Sized + serde::Serialize>(&mut self, value: &T) -> Result<(), Error> {
        // Tuple variant fields have no names; use an empty string as the key.
        self.fields.push((String::new(), value.serialize(Encoder)?));
        Ok(())
    }

    fn end(self) -> Result<TextualNode, Error> {
        Ok(TextualNode::Object(ObjectNode { tag: self.tag, fields: self.fields }))
    }
}

// ── SerializeMap ─────────────────────────────────────────────────────────────

impl ser::SerializeMap for MapEncoder {
    type Ok = TextualNode;
    type Error = Error;

    fn serialize_key<T: ?Sized + serde::Serialize>(&mut self, key: &T) -> Result<(), Error> {
        let key_str = match key.serialize(Encoder)? {
            TextualNode::String(s)  => s.string,
            TextualNode::Numeric(n) => n.int,
            TextualNode::Bool(b)    => b.bool.to_string(),
            _ => return Err(Error("map key must be a scalar".into())),
        };
        self.pending_key = Some(key_str);
        Ok(())
    }

    fn serialize_value<T: ?Sized + serde::Serialize>(&mut self, value: &T) -> Result<(), Error> {
        let key = self.pending_key.take().unwrap_or_default();
        self.fields.push((key, value.serialize(Encoder)?));
        Ok(())
    }

    fn end(self) -> Result<TextualNode, Error> {
        Ok(TextualNode::Object(ObjectNode { tag: "map".to_string(), fields: self.fields }))
    }
}

// ── SerializeStruct ──────────────────────────────────────────────────────────

impl ser::SerializeStruct for StructEncoder {
    type Ok = TextualNode;
    type Error = Error;

    fn serialize_field<T: ?Sized + serde::Serialize>(&mut self, key: &'static str, value: &T) -> Result<(), Error> {
        self.fields.push((key.to_string(), value.serialize(Encoder)?));
        Ok(())
    }

    fn end(self) -> Result<TextualNode, Error> {
        Ok(TextualNode::Object(ObjectNode { tag: self.tag, fields: self.fields }))
    }
}

// ── SerializeStructVariant ───────────────────────────────────────────────────

impl ser::SerializeStructVariant for StructVariantEncoder {
    type Ok = TextualNode;
    type Error = Error;

    fn serialize_field<T: ?Sized + serde::Serialize>(&mut self, key: &'static str, value: &T) -> Result<(), Error> {
        self.fields.push((key.to_string(), value.serialize(Encoder)?));
        Ok(())
    }

    fn end(self) -> Result<TextualNode, Error> {
        Ok(TextualNode::Object(ObjectNode { tag: self.tag, fields: self.fields }))
    }
}

// ── Serializer ───────────────────────────────────────────────────────────────

/// Entry-point serializer. Construct with `Encoder` and call `value.serialize(Encoder)`,
/// or use the [`encode_value`] convenience function.
pub(super) struct Encoder;

impl ser::Serializer for Encoder {
    type Ok = TextualNode;
    type Error = Error;

    type SerializeSeq           = SeqEncoder;
    type SerializeTuple         = SeqEncoder;
    type SerializeTupleStruct   = SeqEncoder;
    type SerializeTupleVariant  = TupleVariantEncoder;
    type SerializeMap           = MapEncoder;
    type SerializeStruct        = StructEncoder;
    type SerializeStructVariant = StructVariantEncoder;

    fn serialize_bool(self, v: bool) -> Result<TextualNode, Error> {
        Ok(TextualNode::Bool(BoolNode { bool: v }))
    }

    fn serialize_i8(self, v: i8) -> Result<TextualNode, Error> {
        Ok(TextualNode::Numeric(NumericNode { int: v.to_string() }))
    }

    fn serialize_i16(self, v: i16) -> Result<TextualNode, Error> {
        Ok(TextualNode::Numeric(NumericNode { int: v.to_string() }))
    }

    fn serialize_i32(self, v: i32) -> Result<TextualNode, Error> {
        Ok(TextualNode::Numeric(NumericNode { int: v.to_string() }))
    }

    fn serialize_i64(self, v: i64) -> Result<TextualNode, Error> {
        Ok(TextualNode::Numeric(NumericNode { int: v.to_string() }))
    }

    fn serialize_i128(self, v: i128) -> Result<TextualNode, Error> {
        Ok(TextualNode::Numeric(NumericNode { int: v.to_string() }))
    }

    fn serialize_u8(self, v: u8) -> Result<TextualNode, Error> {
        Ok(TextualNode::Numeric(NumericNode { int: v.to_string() }))
    }

    fn serialize_u16(self, v: u16) -> Result<TextualNode, Error> {
        Ok(TextualNode::Numeric(NumericNode { int: v.to_string() }))
    }

    fn serialize_u32(self, v: u32) -> Result<TextualNode, Error> {
        Ok(TextualNode::Numeric(NumericNode { int: v.to_string() }))
    }

    fn serialize_u64(self, v: u64) -> Result<TextualNode, Error> {
        Ok(TextualNode::Numeric(NumericNode { int: v.to_string() }))
    }

    fn serialize_u128(self, v: u128) -> Result<TextualNode, Error> {
        Ok(TextualNode::Numeric(NumericNode { int: v.to_string() }))
    }

    fn serialize_f32(self, v: f32) -> Result<TextualNode, Error> {
        Ok(TextualNode::Numeric(NumericNode { int: v.to_string() }))
    }

    fn serialize_f64(self, v: f64) -> Result<TextualNode, Error> {
        Ok(TextualNode::Numeric(NumericNode { int: v.to_string() }))
    }

    fn serialize_char(self, v: char) -> Result<TextualNode, Error> {
        Ok(TextualNode::String(StringNode { string: v.to_string() }))
    }

    fn serialize_str(self, v: &str) -> Result<TextualNode, Error> {
        Ok(TextualNode::String(StringNode { string: v.to_string() }))
    }

    fn serialize_bytes(self, v: &[u8]) -> Result<TextualNode, Error> {
        // Store bytes as an ArrayNode of NumericNodes (each byte as a decimal string).
        let elements = v.iter()
            .map(|b| TextualNode::Numeric(NumericNode { int: b.to_string() }))
            .collect();
        Ok(TextualNode::Array(ArrayNode { elements }))
    }

    fn serialize_none(self) -> Result<TextualNode, Error> {
        // Represent None as a null-tagged empty object.
        Ok(TextualNode::Object(ObjectNode { tag: "null".to_string(), fields: Vec::new() }))
    }

    fn serialize_some<T: ?Sized + serde::Serialize>(self, value: &T) -> Result<TextualNode, Error> {
        // Unwrap Some – serialize the inner value directly.
        value.serialize(self)
    }

    fn serialize_unit(self) -> Result<TextualNode, Error> {
        // Represent () as an empty object tagged "unit".
        Ok(TextualNode::Object(ObjectNode { tag: "unit".to_string(), fields: Vec::new() }))
    }

    fn serialize_unit_struct(self, name: &'static str) -> Result<TextualNode, Error> {
        Ok(TextualNode::Object(ObjectNode { tag: name.to_string(), fields: Vec::new() }))
    }

    fn serialize_unit_variant(self, _name: &'static str, _idx: u32, variant: &'static str) -> Result<TextualNode, Error> {
        // Represent as an object whose tag is the variant name with no fields.
        Ok(TextualNode::Object(ObjectNode { tag: variant.to_string(), fields: Vec::new() }))
    }

    fn serialize_newtype_struct<T: ?Sized + serde::Serialize>(self, _name: &'static str, value: &T) -> Result<TextualNode, Error> {
        // Transparent – serialize the inner value directly.
        value.serialize(self)
    }

    fn serialize_newtype_variant<T: ?Sized + serde::Serialize>(self, _name: &'static str, _idx: u32, variant: &'static str, value: &T) -> Result<TextualNode, Error> {
        // Wrap the inner value in an ObjectNode tagged with the variant name.
        let inner = value.serialize(Encoder)?;
        Ok(TextualNode::Object(ObjectNode {
            tag: variant.to_string(),
            fields: vec![("value".to_string(), inner)],
        }))
    }

    fn serialize_seq(self, len: Option<usize>) -> Result<SeqEncoder, Error> {
        Ok(SeqEncoder { elements: Vec::with_capacity(len.unwrap_or(0)) })
    }

    fn serialize_tuple(self, len: usize) -> Result<SeqEncoder, Error> {
        Ok(SeqEncoder { elements: Vec::with_capacity(len) })
    }

    fn serialize_tuple_struct(self, _name: &'static str, len: usize) -> Result<SeqEncoder, Error> {
        Ok(SeqEncoder { elements: Vec::with_capacity(len) })
    }

    fn serialize_tuple_variant(self, _name: &'static str, _idx: u32, variant: &'static str, len: usize) -> Result<TupleVariantEncoder, Error> {
        Ok(TupleVariantEncoder { tag: variant.to_string(), fields: Vec::with_capacity(len) })
    }

    fn serialize_map(self, len: Option<usize>) -> Result<MapEncoder, Error> {
        Ok(MapEncoder { fields: Vec::with_capacity(len.unwrap_or(0)), pending_key: None })
    }

    fn serialize_struct(self, name: &'static str, len: usize) -> Result<StructEncoder, Error> {
        Ok(StructEncoder { tag: name.to_string(), fields: Vec::with_capacity(len) })
    }

    fn serialize_struct_variant(self, _name: &'static str, _idx: u32, variant: &'static str, len: usize) -> Result<StructVariantEncoder, Error> {
        Ok(StructVariantEncoder { tag: variant.to_string(), fields: Vec::with_capacity(len) })
    }

    fn collect_str<T: ?Sized + fmt::Display>(self, value: &T) -> Result<TextualNode, Error> {
        Ok(TextualNode::String(StringNode { string: value.to_string() }))
    }
}

pub fn encode<T: ?Sized + serde::Serialize>(value: &T) -> Result<String, Error> {
    let node = value.serialize(Encoder)?;
    let mut f = Formatter::new();
    node.format(&mut f);
    Ok(f.out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Serialize;
    use std::collections::HashMap;

    /// Encode any `Serialize` value and return the formatted textual output.
    fn encode_assert<T: Serialize>(value: T) -> String {
        encode(&value).expect("serialization failed")
    }

    // ── Booleans ─────────────────────────────────────────────────────────────

    #[test]
    fn encode_bool_true() {
        assert_eq!(encode_assert(true), "true");
    }

    #[test]
    fn encode_bool_false() {
        assert_eq!(encode_assert(false), "false");
    }

    // ── Integers ─────────────────────────────────────────────────────────────

    #[test]
    fn encode_i8() {
        assert_eq!(encode_assert(-128_i8), "-128");
    }

    #[test]
    fn encode_i16() {
        assert_eq!(encode_assert(-32768_i16), "-32768");
    }

    #[test]
    fn encode_i32() {
        assert_eq!(encode_assert(42_i32), "42");
    }

    #[test]
    fn encode_i64() {
        assert_eq!(encode_assert(-1_i64), "-1");
    }

    #[test]
    fn encode_i128() {
        assert_eq!(encode_assert(i128::MAX), i128::MAX.to_string());
    }

    #[test]
    fn encode_u8() {
        assert_eq!(encode_assert(255_u8), "255");
    }

    #[test]
    fn encode_u16() {
        assert_eq!(encode_assert(1000_u16), "1000");
    }

    #[test]
    fn encode_u32() {
        assert_eq!(encode_assert(0_u32), "0");
    }

    #[test]
    fn encode_u64() {
        assert_eq!(encode_assert(u64::MAX), u64::MAX.to_string());
    }

    #[test]
    fn encode_u128() {
        assert_eq!(encode_assert(u128::MAX), u128::MAX.to_string());
    }

    // ── Floats ────────────────────────────────────────────────────────────────

    #[test]
    fn encode_f32() {
        assert_eq!(encode_assert(1.5_f32), 1.5_f32.to_string());
    }

    #[test]
    fn encode_f64() {
        assert_eq!(encode_assert(3.14_f64), 3.14_f64.to_string());
    }

    // ── Char & str ────────────────────────────────────────────────────────────

    #[test]
    fn encode_char() {
        assert_eq!(encode_assert('z'), "\"z\"");
    }

    #[test]
    fn encode_str() {
        assert_eq!(encode_assert("hello"), "\"hello\"");
    }

    #[test]
    fn encode_str_with_escapes() {
        assert_eq!(encode_assert("a\nb"), "\"a\\nb\"");
    }

    // ── Bytes ─────────────────────────────────────────────────────────────────

    #[test]
    fn encode_bytes() {
        struct Bytes<'a>(&'a [u8]);
        impl<'a> Serialize for Bytes<'a> {
            fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
                s.serialize_bytes(self.0)
            }
        }
        assert_eq!(encode_assert(Bytes(&[1, 2, 3])), "[\n  1,\n  2,\n  3\n]");
    }

    #[test]
    fn encode_bytes_empty() {
        struct Bytes<'a>(&'a [u8]);
        impl<'a> Serialize for Bytes<'a> {
            fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
                s.serialize_bytes(self.0)
            }
        }
        assert_eq!(encode_assert(Bytes(&[])), "[]");
    }

    // ── Option ────────────────────────────────────────────────────────────────

    #[test]
    fn encode_none() {
        assert_eq!(encode_assert(None::<i32>), "null");
    }

    #[test]
    fn encode_some() {
        assert_eq!(encode_assert(Some(7_i32)), "7");
    }

    #[test]
    fn encode_some_string() {
        assert_eq!(encode_assert(Some("hi")), "\"hi\"");
    }

    // ── Unit ──────────────────────────────────────────────────────────────────

    #[test]
    fn encode_unit() {
        assert_eq!(encode_assert(()), "unit");
    }

    // ── Unit struct ───────────────────────────────────────────────────────────

    #[test]
    fn encode_unit_struct() {
        #[derive(Serialize)]
        struct Marker;
        assert_eq!(encode_assert(Marker), "Marker");
    }

    // ── Unit variant ──────────────────────────────────────────────────────────

    #[test]
    fn encode_unit_variant() {
        #[derive(Serialize)]
        enum Direction { North, South }
        assert_eq!(encode_assert(Direction::North), "North");
        assert_eq!(encode_assert(Direction::South), "South");
    }

    // ── Newtype struct ────────────────────────────────────────────────────────

    #[test]
    fn encode_newtype_struct() {
        #[derive(Serialize)]
        struct Meters(f64);
        // Transparent: inner value is serialized directly.
        assert_eq!(encode_assert(Meters(1.5)), 1.5_f64.to_string());
    }

    // ── Newtype variant ───────────────────────────────────────────────────────

    #[test]
    fn encode_newtype_variant() {
        #[derive(Serialize)]
        enum Wrapper { Val(i32) }
        assert_eq!(encode_assert(Wrapper::Val(99)), "Val {\n  value: 99\n}");
    }

    #[test]
    fn encode_newtype_variant_string() {
        #[derive(Serialize)]
        enum Wrapper { Name(String) }
        assert_eq!(encode_assert(Wrapper::Name("Alice".into())), "Name {\n  value: \"Alice\"\n}");
    }

    // ── Sequence ──────────────────────────────────────────────────────────────

    #[test]
    fn encode_seq_empty() {
        let v: Vec<i32> = vec![];
        assert_eq!(encode_assert(v), "[]");
    }

    #[test]
    fn encode_seq_ints() {
        assert_eq!(encode_assert(vec![1_i32, 2, 3]), "[\n  1,\n  2,\n  3\n]");
    }

    #[test]
    fn encode_seq_strings() {
        assert_eq!(encode_assert(vec!["a", "b"]), "[\n  \"a\",\n  \"b\"\n]");
    }

    #[test]
    fn encode_seq_bools() {
        assert_eq!(encode_assert(vec![true, false]), "[\n  true,\n  false\n]");
    }

    // ── Tuple ─────────────────────────────────────────────────────────────────

    #[test]
    fn encode_tuple() {
        assert_eq!(encode_assert((1_i32, "x")), "[\n  1,\n  \"x\"\n]");
    }

    #[test]
    fn encode_tuple_single() {
        assert_eq!(encode_assert((42_i32,)), "[\n  42\n]");
    }

    // ── Tuple struct ──────────────────────────────────────────────────────────

    #[test]
    fn encode_tuple_struct() {
        #[derive(Serialize)]
        struct Pair(i32, i32);
        assert_eq!(encode_assert(Pair(3, 4)), "[\n  3,\n  4\n]");
    }

    // ── Tuple variant ─────────────────────────────────────────────────────────

    #[test]
    fn encode_tuple_variant() {
        #[derive(Serialize)]
        enum Shape { Point(i32, i32) }
        // Tuple variant fields land in an ObjectNode keyed by empty strings.
        assert_eq!(encode_assert(Shape::Point(1, 2)), "Point {\n  : 1,\n  : 2\n}");
    }

    // ── Map ───────────────────────────────────────────────────────────────────

    #[test]
    fn encode_map_empty() {
        let m: HashMap<String, i32> = HashMap::new();
        assert_eq!(encode_assert(m), "map");
    }

    #[test]
    fn encode_map_single_entry() {
        let mut m = HashMap::new();
        m.insert("key", 1_i32);
        assert_eq!(encode_assert(m), "map {\n  key: 1\n}");
    }

    // ── Struct ────────────────────────────────────────────────────────────────

    #[test]
    fn encode_struct_single_field() {
        #[derive(Serialize)]
        struct Point { x: i32 }
        assert_eq!(encode_assert(Point { x: 10 }), "Point {\n  x: 10\n}");
    }

    #[test]
    fn encode_struct_multiple_fields() {
        #[derive(Serialize)]
        struct Point { x: i32, y: i32 }
        assert_eq!(encode_assert(Point { x: 3, y: 4 }), "Point {\n  x: 3,\n  y: 4\n}");
    }

    #[test]
    fn encode_struct_mixed_types() {
        #[derive(Serialize)]
        struct Person { name: String, age: u32, active: bool }
        assert_eq!(
            encode_assert(Person { name: "Alice".into(), age: 30, active: true }),
            "Person {\n  name: \"Alice\",\n  age: 30,\n  active: true\n}"
        );
    }

    // ── Struct variant ────────────────────────────────────────────────────────

    #[test]
    fn encode_struct_variant() {
        #[derive(Serialize)]
        enum Event { Click { x: i32, y: i32 } }
        assert_eq!(encode_assert(Event::Click { x: 5, y: 6 }), "Click {\n  x: 5,\n  y: 6\n}");
    }

    // ── Nested structures ─────────────────────────────────────────────────────

    #[test]
    fn encode_struct_with_vec_field() {
        #[derive(Serialize)]
        struct Bag { items: Vec<i32> }
        assert_eq!(
            encode_assert(Bag { items: vec![1, 2, 3] }),
            "Bag {\n  items: [\n    1,\n    2,\n    3\n  ]\n}"
        );
    }

    #[test]
    fn encode_nested_structs() {
        #[derive(Serialize)]
        struct Inner { val: i32 }
        #[derive(Serialize)]
        struct Outer { child: Inner }
        assert_eq!(
            encode_assert(Outer { child: Inner { val: 99 } }),
            "Outer {\n  child: Inner {\n    val: 99\n  }\n}"
        );
    }

    #[test]
    fn encode_vec_of_structs() {
        #[derive(Serialize)]
        struct Item { id: i32 }
        assert_eq!(
            encode_assert(vec![Item { id: 1 }, Item { id: 2 }]),
            "[\n  Item {\n    id: 1\n  },\n  Item {\n    id: 2\n  }\n]"
        );
    }

    #[test]
    fn encode_option_in_struct() {
        #[derive(Serialize)]
        struct Opt { val: Option<i32> }
        assert_eq!(encode_assert(Opt { val: None }), "Opt {\n  val: null\n}");
        assert_eq!(encode_assert(Opt { val: Some(5) }), "Opt {\n  val: 5\n}");
    }

    #[test]
    fn encode_vec_of_options() {
        assert_eq!(
            encode_assert(vec![Some(1_i32), None, Some(3)]),
            "[\n  1,\n  null,\n  3\n]"
        );
    }
}
