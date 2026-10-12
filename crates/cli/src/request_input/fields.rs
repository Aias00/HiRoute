//! Check object member names at the CLI boundary without tightening persisted DTO readers.
//!
//! Request DTOs use matching Serialize/Deserialize field names (no aliases or flatten).
//! Serialize exposes both emitted and skipped struct fields, including buffered tagged
//! variants that can hide ignored fields from a deserializer wrapper. Maps and JSON Values
//! retain their free keys; typed map values and sequence elements are checked recursively.
//! Serde's accepted positional struct arrays use the same declared field order.

use serde::Serialize;
use serde::ser::{self, SerializeMap, SerializeSeq, SerializeStruct};
use serde_json::{Error, Value};

pub(super) fn validate<T: ?Sized + Serialize>(request: &T, source: &Value) -> Result<(), Error> {
    request.serialize(Fields(Some(source)))
}

struct Fields<'a>(Option<&'a Value>);

struct Object<'a> {
    source: Option<&'a Value>,
    fields: Vec<&'static str>,
}

struct Sequence<'a> {
    source: Option<&'a Value>,
    index: usize,
}

struct Map<'a> {
    source: Option<&'a Value>,
    key: Option<String>,
}

macro_rules! scalar {
    ($($method:ident($type:ty)),+ $(,)?) => {$(
        fn $method(self, _value: $type) -> Result<(), Error> { Ok(()) }
    )+};
}

impl<'a> ser::Serializer for Fields<'a> {
    type Ok = ();
    type Error = Error;
    type SerializeSeq = Sequence<'a>;
    type SerializeTuple = Sequence<'a>;
    type SerializeTupleStruct = Sequence<'a>;
    type SerializeTupleVariant = Sequence<'a>;
    type SerializeMap = Map<'a>;
    type SerializeStruct = Object<'a>;
    type SerializeStructVariant = Object<'a>;

    scalar!(
        serialize_bool(bool),
        serialize_i8(i8),
        serialize_i16(i16),
        serialize_i32(i32),
        serialize_i64(i64),
        serialize_i128(i128),
        serialize_u8(u8),
        serialize_u16(u16),
        serialize_u32(u32),
        serialize_u64(u64),
        serialize_u128(u128),
        serialize_f32(f32),
        serialize_f64(f64),
        serialize_char(char),
        serialize_str(&str),
        serialize_bytes(&[u8])
    );

    fn serialize_none(self) -> Result<(), Error> {
        Ok(())
    }
    fn serialize_unit(self) -> Result<(), Error> {
        Ok(())
    }
    fn serialize_unit_struct(self, _name: &'static str) -> Result<(), Error> {
        Ok(())
    }
    fn serialize_unit_variant(
        self,
        _name: &'static str,
        _index: u32,
        _variant: &'static str,
    ) -> Result<(), Error> {
        Ok(())
    }

    fn serialize_some<T: ?Sized + Serialize>(self, value: &T) -> Result<(), Error> {
        value.serialize(self)
    }

    fn serialize_newtype_struct<T: ?Sized + Serialize>(
        self,
        _name: &'static str,
        value: &T,
    ) -> Result<(), Error> {
        value.serialize(self)
    }

    fn serialize_newtype_variant<T: ?Sized + Serialize>(
        self,
        _name: &'static str,
        _index: u32,
        variant: &'static str,
        value: &T,
    ) -> Result<(), Error> {
        value.serialize(Fields(self.0.and_then(|source| source.get(variant))))
    }

    fn serialize_seq(self, _len: Option<usize>) -> Result<Sequence<'a>, Error> {
        Ok(Sequence {
            source: self.0,
            index: 0,
        })
    }
    fn serialize_tuple(self, len: usize) -> Result<Sequence<'a>, Error> {
        self.serialize_seq(Some(len))
    }
    fn serialize_tuple_struct(
        self,
        _name: &'static str,
        len: usize,
    ) -> Result<Sequence<'a>, Error> {
        self.serialize_tuple(len)
    }
    fn serialize_tuple_variant(
        self,
        _name: &'static str,
        _index: u32,
        variant: &'static str,
        len: usize,
    ) -> Result<Sequence<'a>, Error> {
        Fields(self.0.and_then(|source| source.get(variant))).serialize_tuple(len)
    }

    fn serialize_map(self, _len: Option<usize>) -> Result<Map<'a>, Error> {
        Ok(Map {
            source: self.0,
            key: None,
        })
    }
    fn serialize_struct(self, _name: &'static str, len: usize) -> Result<Object<'a>, Error> {
        Ok(Object {
            source: self.0,
            fields: Vec::with_capacity(len),
        })
    }
    fn serialize_struct_variant(
        self,
        name: &'static str,
        _index: u32,
        variant: &'static str,
        len: usize,
    ) -> Result<Object<'a>, Error> {
        Fields(self.0.and_then(|source| source.get(variant))).serialize_struct(name, len)
    }
}

impl SerializeSeq for Sequence<'_> {
    type Ok = ();
    type Error = Error;
    fn serialize_element<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), Error> {
        if let Some(source) = self.source.and_then(|source| source.get(self.index)) {
            validate(value, source)?;
        }
        self.index += 1;
        Ok(())
    }
    fn end(self) -> Result<(), Error> {
        Ok(())
    }
}

macro_rules! sequence {
    ($($trait:ident => $method:ident),+ $(,)?) => {$(
        impl ser::$trait for Sequence<'_> {
            type Ok = ();
            type Error = Error;
            fn $method<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), Error> {
                SerializeSeq::serialize_element(self, value)
            }
            fn end(self) -> Result<(), Error> { Ok(()) }
        }
    )+};
}
sequence!(
    SerializeTuple => serialize_element,
    SerializeTupleStruct => serialize_field,
    SerializeTupleVariant => serialize_field,
);

impl SerializeMap for Map<'_> {
    type Ok = ();
    type Error = Error;
    fn serialize_key<T: ?Sized + Serialize>(&mut self, key: &T) -> Result<(), Error> {
        self.key = Some(match serde_json::to_value(key)? {
            Value::String(key) => key,
            key @ (Value::Number(_) | Value::Bool(_)) => key.to_string(),
            _ => return Err(ser::Error::custom("invalid JSON object key")),
        });
        Ok(())
    }
    fn serialize_value<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), Error> {
        let key = self
            .key
            .take()
            .ok_or_else(|| <Error as ser::Error>::custom("missing map key"))?;
        if let Some(source) = self.source.and_then(|source| source.get(&key)) {
            validate(value, source)?;
        }
        Ok(())
    }
    fn end(self) -> Result<(), Error> {
        Ok(())
    }
}

impl SerializeStruct for Object<'_> {
    type Ok = ();
    type Error = Error;
    fn serialize_field<T: ?Sized + Serialize>(
        &mut self,
        key: &'static str,
        value: &T,
    ) -> Result<(), Error> {
        let index = self.fields.len();
        self.fields.push(key);
        if let Some(source) = self.source.and_then(|source| match source {
            Value::Array(values) => values.get(index),
            _ => source.get(key),
        }) {
            validate(value, source)?;
        }
        Ok(())
    }
    fn skip_field(&mut self, key: &'static str) -> Result<(), Error> {
        self.fields.push(key);
        Ok(())
    }
    fn end(self) -> Result<(), Error> {
        if self
            .source
            .and_then(Value::as_object)
            .is_some_and(|source| {
                source
                    .keys()
                    .any(|key| !self.fields.contains(&key.as_str()))
            })
        {
            return Err(ser::Error::custom("unknown request field"));
        }
        Ok(())
    }
}

impl ser::SerializeStructVariant for Object<'_> {
    type Ok = ();
    type Error = Error;
    fn serialize_field<T: ?Sized + Serialize>(
        &mut self,
        key: &'static str,
        value: &T,
    ) -> Result<(), Error> {
        SerializeStruct::serialize_field(self, key, value)
    }
    fn skip_field(&mut self, key: &'static str) -> Result<(), Error> {
        SerializeStruct::skip_field(self, key)
    }
    fn end(self) -> Result<(), Error> {
        SerializeStruct::end(self)
    }
}
