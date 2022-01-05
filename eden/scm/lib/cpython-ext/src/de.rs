/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use cpython::*;
use serde::de;
use serde::de::Visitor;

use crate::PyErr as Error;
use crate::PyNone;

type Result<T> = std::result::Result<T, Error>;

/// Deserialize from Python object.
pub fn from_object<'de, T>(py: Python, obj: PyObject) -> Result<T>
where
    T: de::Deserialize<'de>,
{
    let mut deserializer = Deserializer::new(py, obj);
    T::deserialize(&mut deserializer)
}

struct Deserializer<'gil> {
    py: Python<'gil>,
    obj: PyObject,

    /// Iterator of `obj`. Constructed on demand.
    obj_iter: Option<PyIterator<'gil>>,

    /// Used by `MapAccess`. The `iter` produces `(key, value)` at a time, the
    /// `value` is temporarily stored here for `MapAccess::next_value_seed` to
    /// pick up.
    pending_value: Option<PyObject>,
}

impl<'gil> Deserializer<'gil> {
    /// Constructs from a Python object.
    fn new(py: Python<'gil>, obj: PyObject) -> Self {
        Self {
            py,
            obj,
            obj_iter: None,
            pending_value: None,
        }
    }

    /// Returns the next item from a Python iterator (obj should be an iterable).
    fn next(&mut self) -> Result<Option<PyObject>> {
        match self.obj_iter {
            None => {
                // Convert `obj` to a Python iterator object.
                // Usually this is a no-op for a type that is already an iterator.
                //
                // Special case: for PyDict, call the "items" method first to get
                // an iterator of (key, value) instead of just keys.
                let iter = if self.extract::<PyDict>().is_ok() {
                    let items = self.obj.call_method(self.py, "items", NoArgs, None)?;
                    items.iter(self.py)?
                } else {
                    self.obj.iter(self.py)?
                };
                self.obj_iter = Some(iter);
                self.next()
            }
            Some(ref mut iter) => iter.next().transpose().map_err(Into::into),
        }
    }

    /// Attempt to convert `obj` to a given type.
    fn extract<T>(&self) -> Result<T>
    where
        for<'s> T: FromPyObject<'s>,
    {
        self.obj.extract(self.py).map_err(Into::into)
    }
}

impl<'de, 'a, 'gil> de::Deserializer<'de> for &'a mut Deserializer<'gil> {
    type Error = Error;

    fn deserialize_any<V: Visitor<'de>>(self, v: V) -> Result<V::Value> {
        if self.extract::<PyNone>().is_ok() {
            v.visit_none()
        } else if self.extract::<bool>().is_ok() {
            self.deserialize_bool(v)
        } else if self.extract::<PyDict>().is_ok() {
            self.deserialize_map(v)
        } else if self.extract::<PyList>().is_ok() || self.extract::<PyTuple>().is_ok() {
            self.deserialize_seq(v)
        } else if self.extract::<PyBytes>().is_ok() {
            self.deserialize_bytes(v)
        } else if self.extract::<PyString>().is_ok() {
            self.deserialize_string(v)
        } else if self.extract::<i64>().is_ok() {
            self.deserialize_i64(v)
        } else if self.extract::<u64>().is_ok() {
            self.deserialize_u64(v)
        } else if self.extract::<f64>().is_ok() {
            self.deserialize_f64(v)
        } else {
            Err(PyErr::new::<exc::TypeError, _>(
                self.py,
                "deserialize_any encountered undecided type",
            )
            .into())
        }
    }

    // Code generation for simple types.
    // Run with: python -m cogapp -r src/de.rs
    //
    // [[[cog
    // import cog
    // for name in 'bool i8 i16 i32 i64 u8 u16 u32 u64 f32 f64 string'.split():
    //     cog.out("""
    //     fn deserialize_{name}<V: Visitor<'de>>(self, v: V) -> Result<V::Value> {{
    //         v.visit_{name}(self.extract()?)
    //     }}
    // """.format(name=name))
    // ]]]

    fn deserialize_bool<V: Visitor<'de>>(self, v: V) -> Result<V::Value> {
        v.visit_bool(self.extract()?)
    }

    fn deserialize_i8<V: Visitor<'de>>(self, v: V) -> Result<V::Value> {
        v.visit_i8(self.extract()?)
    }

    fn deserialize_i16<V: Visitor<'de>>(self, v: V) -> Result<V::Value> {
        v.visit_i16(self.extract()?)
    }

    fn deserialize_i32<V: Visitor<'de>>(self, v: V) -> Result<V::Value> {
        v.visit_i32(self.extract()?)
    }

    fn deserialize_i64<V: Visitor<'de>>(self, v: V) -> Result<V::Value> {
        v.visit_i64(self.extract()?)
    }

    fn deserialize_u8<V: Visitor<'de>>(self, v: V) -> Result<V::Value> {
        v.visit_u8(self.extract()?)
    }

    fn deserialize_u16<V: Visitor<'de>>(self, v: V) -> Result<V::Value> {
        v.visit_u16(self.extract()?)
    }

    fn deserialize_u32<V: Visitor<'de>>(self, v: V) -> Result<V::Value> {
        v.visit_u32(self.extract()?)
    }

    fn deserialize_u64<V: Visitor<'de>>(self, v: V) -> Result<V::Value> {
        v.visit_u64(self.extract()?)
    }

    fn deserialize_f32<V: Visitor<'de>>(self, v: V) -> Result<V::Value> {
        v.visit_f32(self.extract()?)
    }

    fn deserialize_f64<V: Visitor<'de>>(self, v: V) -> Result<V::Value> {
        v.visit_f64(self.extract()?)
    }

    fn deserialize_string<V: Visitor<'de>>(self, v: V) -> Result<V::Value> {
        v.visit_string(self.extract()?)
    }
    // [[[end]]]

    fn deserialize_char<V: Visitor<'de>>(self, v: V) -> Result<V::Value> {
        self.deserialize_string(v)
    }

    fn deserialize_str<V: Visitor<'de>>(self, v: V) -> Result<V::Value> {
        self.deserialize_string(v)
    }

    fn deserialize_bytes<V: Visitor<'de>>(self, v: V) -> Result<V::Value> {
        if self.obj.get_type(self.py) == PyString::type_object(self.py) {
            let s: String = self.extract()?;
            v.visit_bytes(s.as_bytes())
        } else {
            let pybytes: PyBytes = self.extract()?;
            v.visit_bytes(pybytes.data(self.py))
        }
    }

    fn deserialize_byte_buf<V: Visitor<'de>>(self, v: V) -> Result<V::Value> {
        self.deserialize_bytes(v)
    }

    fn deserialize_option<V: Visitor<'de>>(self, v: V) -> Result<V::Value> {
        if let Ok(PyNone) = self.extract() {
            v.visit_none()
        } else {
            v.visit_some(self)
        }
    }

    fn deserialize_unit<V: Visitor<'de>>(self, v: V) -> Result<V::Value> {
        self.extract::<PyNone>()?;
        v.visit_unit()
    }

    fn deserialize_unit_struct<V: Visitor<'de>>(self, _: &'static str, v: V) -> Result<V::Value> {
        self.deserialize_unit(v)
    }

    fn deserialize_newtype_struct<V: Visitor<'de>>(
        self,
        _: &'static str,
        v: V,
    ) -> Result<V::Value> {
        v.visit_newtype_struct(self)
    }

    fn deserialize_seq<V: Visitor<'de>>(self, v: V) -> Result<V::Value> {
        v.visit_seq(self)
    }

    fn deserialize_tuple<V: Visitor<'de>>(self, _len: usize, v: V) -> Result<V::Value> {
        self.deserialize_seq(v)
    }

    fn deserialize_tuple_struct<V: Visitor<'de>>(
        self,
        _name: &'static str,
        _len: usize,
        v: V,
    ) -> Result<V::Value> {
        self.deserialize_seq(v)
    }

    fn deserialize_map<V: Visitor<'de>>(self, v: V) -> Result<V::Value> {
        v.visit_map(self)
    }

    fn deserialize_struct<V: Visitor<'de>>(
        self,
        _name: &'static str,
        _fields: &'static [&'static str],
        v: V,
    ) -> Result<V::Value> {
        self.deserialize_map(v)
    }

    fn deserialize_enum<V: Visitor<'de>>(
        self,
        _name: &'static str,
        _variants: &'static [&'static str],
        v: V,
    ) -> Result<V::Value> {
        v.visit_enum(self)
    }

    fn deserialize_identifier<V: Visitor<'de>>(self, v: V) -> Result<V::Value> {
        self.deserialize_string(v)
    }

    fn deserialize_ignored_any<V: Visitor<'de>>(self, v: V) -> Result<V::Value> {
        self.deserialize_any(v)
    }
}

impl<'de, 'a, 'gil> de::SeqAccess<'de> for &'a mut Deserializer<'gil> {
    type Error = Error;

    fn next_element_seed<T>(&mut self, seed: T) -> Result<Option<T::Value>>
    where
        T: de::DeserializeSeed<'de>,
    {
        match self.next()? {
            Some(obj) => {
                let mut deserializer = Deserializer::new(self.py, obj);
                seed.deserialize(&mut deserializer).map(Some)
            }
            None => Ok(None),
        }
    }
}

impl<'de, 'a, 'gil> de::MapAccess<'de> for &'a mut Deserializer<'gil> {
    type Error = Error;

    fn next_key_seed<K>(&mut self, seed: K) -> Result<Option<K::Value>>
    where
        K: de::DeserializeSeed<'de>,
    {
        match self.next()? {
            Some(obj) => {
                let (key, value): (PyObject, PyObject) = obj.extract(self.py)?;
                self.pending_value = Some(value);
                let mut deserializer = Deserializer::new(self.py, key);
                seed.deserialize(&mut deserializer).map(Some)
            }
            None => Ok(None),
        }
    }

    fn next_value_seed<V>(&mut self, seed: V) -> Result<V::Value>
    where
        V: de::DeserializeSeed<'de>,
    {
        match self.pending_value.take() {
            Some(obj) => {
                let mut deserializer = Deserializer::new(self.py, obj);
                seed.deserialize(&mut deserializer)
            }
            None => Err(PyErr::new::<exc::ValueError, _>(
                self.py,
                "no value for MapAccess::next_value_seed to pick up",
            )
            .into()),
        }
    }
}

impl<'de, 'a, 'gil> de::EnumAccess<'de> for &'a mut Deserializer<'gil> {
    type Error = Error;
    type Variant = Deserializer<'gil>;

    fn variant_seed<V>(self, seed: V) -> Result<(V::Value, Self::Variant)>
    where
        V: de::DeserializeSeed<'de>,
    {
        if self.extract::<String>().is_ok() {
            // a string for unit enum variants.
            let name = seed.deserialize(&mut *self)?;
            Ok((name, Deserializer::new(self.py, self.py.None())))
        } else {
            // a dict for complex enum variants.
            let dict: PyDict = self.extract()?;
            let items: Vec<(PyObject, PyObject)> = dict.items(self.py);
            if items.len() != 1 {
                let repr = self.obj.repr(self.py)?;
                let repr = repr.to_string_lossy(self.py);
                let msg = format!("dict for enum should only contain 1 item: {}", repr);
                return Err(PyErr::new::<exc::ValueError, _>(self.py, msg).into());
            }
            let (key, value) = items.into_iter().next().unwrap();
            let name = seed.deserialize(&mut Deserializer::new(self.py, key))?;
            Ok((name, Deserializer::new(self.py, value)))
        }
    }
}

impl<'de, 'gil> de::VariantAccess<'de> for Deserializer<'gil> {
    type Error = Error;

    fn unit_variant(self) -> Result<()> {
        self.extract::<PyNone>()?;
        Ok(())
    }

    fn newtype_variant_seed<T>(mut self, seed: T) -> Result<T::Value>
    where
        T: de::DeserializeSeed<'de>,
    {
        seed.deserialize(&mut self)
    }

    fn tuple_variant<V>(mut self, _len: usize, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        de::Deserializer::deserialize_seq(&mut self, visitor)
    }

    fn struct_variant<V>(mut self, _fields: &'static [&'static str], visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        de::Deserializer::deserialize_map(&mut self, visitor)
    }
}
