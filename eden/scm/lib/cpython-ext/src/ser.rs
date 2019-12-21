/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::PyErr as Error;
use cpython::*;
use serde::{ser, Serialize};

/// Serialize into Python object.
pub fn to_object<T>(py: Python, value: &T) -> PyResult<PyObject>
where
    T: Serialize,
{
    let serializer = Serializer { py };
    value.serialize(&serializer).map_err(|e| e.into_inner())
}

// ------- Serializer Types -------

type Result<T> = std::result::Result<T, Error>;

struct Serializer<'a> {
    py: Python<'a>,
}

trait PyDefault<'a> {
    fn default(py: Python<'a>) -> Self;
}

trait PyCollect<'a> {
    fn collect(self) -> Result<PyObject>;
}

impl<'a> PyDefault<'a> for Serializer<'a> {
    fn default(py: Python<'a>) -> Self {
        Self { py }
    }
}

macro_rules! define_serializer {
    ($name: tt) => {
        struct $name<'a> {
            py: Python<'a>,
            items: Vec<PyObject>,
        }

        impl<'a> PyDefault<'a> for $name<'a> {
            fn default(py: Python<'a>) -> Self {
                Self {
                    py,
                    items: Vec::default(),
                }
            }
        }
    };
}

define_serializer!(ListSerializer);
define_serializer!(MapSerializer);
define_serializer!(TupleSerializer);

impl<'a> PyCollect<'a> for ListSerializer<'a> {
    fn collect(self) -> Result<PyObject> {
        Ok(self.items.into_py_object(self.py).into_object())
    }
}

impl<'a> PyCollect<'a> for TupleSerializer<'a> {
    fn collect(self) -> Result<PyObject> {
        Ok(PyTuple::new(self.py, &self.items).into_object())
    }
}

impl<'a> PyCollect<'a> for MapSerializer<'a> {
    fn collect(self) -> Result<PyObject> {
        let dict = PyDict::new(self.py);
        for chunk in self.items.chunks(2) {
            if let [key, value] = chunk {
                dict.set_item(self.py, key, value)?;
            }
        }
        Ok(dict.into_object())
    }
}

// ------- Serde APIs -------

impl<'a> Serializer<'a> {
    fn serialize<T: ToPyObject>(&self, obj: T) -> Result<PyObject> {
        Ok(obj.into_py_object(self.py).into_object())
    }

    fn to_object<T: Serialize + ?Sized>(py: Python, value: &T) -> Result<PyObject> {
        let serializer = Serializer { py };
        value.serialize(&serializer)
    }
}

impl<'a, 'b> ser::Serializer for &'a Serializer<'b> {
    type Ok = PyObject;
    type Error = Error;

    type SerializeSeq = ListSerializer<'b>;
    type SerializeTuple = TupleSerializer<'b>;
    type SerializeTupleStruct = TupleSerializer<'b>;
    type SerializeTupleVariant = TupleSerializer<'b>;
    type SerializeMap = MapSerializer<'b>;
    type SerializeStruct = MapSerializer<'b>;
    type SerializeStructVariant = MapSerializer<'b>;

    fn serialize_bool(self, v: bool) -> Result<PyObject> {
        self.serialize(v)
    }

    fn serialize_i8(self, v: i8) -> Result<PyObject> {
        self.serialize(v)
    }

    fn serialize_i16(self, v: i16) -> Result<PyObject> {
        self.serialize(v)
    }

    fn serialize_i32(self, v: i32) -> Result<PyObject> {
        self.serialize(v)
    }

    fn serialize_i64(self, v: i64) -> Result<PyObject> {
        self.serialize(v)
    }

    fn serialize_u8(self, v: u8) -> Result<PyObject> {
        self.serialize(v)
    }

    fn serialize_u16(self, v: u16) -> Result<PyObject> {
        self.serialize(v)
    }

    fn serialize_u32(self, v: u32) -> Result<PyObject> {
        self.serialize(v)
    }

    fn serialize_u64(self, v: u64) -> Result<PyObject> {
        self.serialize(v)
    }

    fn serialize_f32(self, v: f32) -> Result<PyObject> {
        self.serialize(v)
    }

    fn serialize_f64(self, v: f64) -> Result<PyObject> {
        self.serialize(v)
    }

    fn serialize_char(self, v: char) -> Result<PyObject> {
        self.serialize_str(&v.to_string())
    }

    fn serialize_str(self, v: &str) -> Result<PyObject> {
        self.serialize_bytes(v.as_bytes())
    }

    fn serialize_bytes(self, v: &[u8]) -> Result<PyObject> {
        Ok(PyBytes::new(self.py, v).into_object())
    }

    fn serialize_none(self) -> Result<PyObject> {
        Ok(self.py.None())
    }

    fn serialize_some<T>(self, value: &T) -> Result<PyObject>
    where
        T: ?Sized + Serialize,
    {
        value.serialize(self)
    }

    fn serialize_unit(self) -> Result<PyObject> {
        self.serialize_none()
    }

    fn serialize_unit_struct(self, _name: &'static str) -> Result<PyObject> {
        self.serialize_unit()
    }

    fn serialize_unit_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        variant: &'static str,
    ) -> Result<PyObject> {
        self.serialize_str(variant)
    }

    fn serialize_newtype_struct<T>(self, _name: &'static str, value: &T) -> Result<PyObject>
    where
        T: ?Sized + Serialize,
    {
        value.serialize(self)
    }

    fn serialize_newtype_variant<T>(
        self,
        _name: &'static str,
        _variant_index: u32,
        variant: &'static str,
        value: &T,
    ) -> Result<PyObject>
    where
        T: ?Sized + Serialize,
    {
        // Serde JSON example serialize this into `{ NAME: VALUE }`.
        // Do something similar.
        let dict = PyDict::new(self.py);
        let key = variant.serialize(self)?;
        let value = value.serialize(self)?;
        dict.set_item(self.py, key, value)?;
        Ok(dict.into_object())
    }

    fn serialize_seq(self, _len: Option<usize>) -> Result<Self::SerializeSeq> {
        Ok(PyDefault::default(self.py))
    }

    fn serialize_tuple(self, _len: usize) -> Result<Self::SerializeTuple> {
        Ok(PyDefault::default(self.py))
    }

    fn serialize_tuple_struct(
        self,
        _name: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeTupleStruct> {
        Ok(PyDefault::default(self.py))
    }

    fn serialize_tuple_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        _variant: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeTupleVariant> {
        Ok(PyDefault::default(self.py))
    }

    fn serialize_map(self, _len: Option<usize>) -> Result<Self::SerializeMap> {
        Ok(PyDefault::default(self.py))
    }

    fn serialize_struct(self, _name: &'static str, _len: usize) -> Result<Self::SerializeStruct> {
        Ok(PyDefault::default(self.py))
    }

    fn serialize_struct_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        _variant: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeStructVariant> {
        Ok(PyDefault::default(self.py))
    }
}

macro_rules! impl_seq {
    ($trait: ty,  $name: tt) => {
        impl_seq!($trait, $name, ListSerializer);
        impl_seq!($trait, $name, TupleSerializer);
        impl_seq!($trait, $name, MapSerializer);
    };

    ($trait: ty,  $name: tt, $type: tt) => {
        impl<'a> $trait for $type<'_> {
            type Ok = PyObject;
            type Error = Error;

            fn $name<T>(&mut self, value: &T) -> Result<()>
            where
                T: ?Sized + Serialize,
            {
                let obj = Serializer::to_object(self.py, value)?;
                self.items.push(obj);
                Ok(())
            }

            fn end(self) -> Result<PyObject> {
                PyCollect::collect(self)
            }
        }
    };
}

impl_seq!(ser::SerializeSeq, serialize_element);
impl_seq!(ser::SerializeTuple, serialize_element);
impl_seq!(ser::SerializeTupleStruct, serialize_field);
impl_seq!(ser::SerializeTupleVariant, serialize_field);

impl<'a> ser::SerializeMap for MapSerializer<'_> {
    type Ok = PyObject;
    type Error = Error;

    fn serialize_key<T>(&mut self, key: &T) -> Result<()>
    where
        T: ?Sized + Serialize,
    {
        let obj = Serializer::to_object(self.py, key)?;
        self.items.push(obj);
        Ok(())
    }

    fn serialize_value<T>(&mut self, value: &T) -> Result<()>
    where
        T: ?Sized + Serialize,
    {
        let obj = Serializer::to_object(self.py, value)?;
        self.items.push(obj);
        Ok(())
    }

    fn end(self) -> Result<PyObject> {
        PyCollect::collect(self)
    }
}

impl<'a> ser::SerializeStruct for MapSerializer<'_> {
    type Ok = PyObject;
    type Error = Error;

    fn serialize_field<T>(&mut self, key: &'static str, value: &T) -> Result<()>
    where
        T: ?Sized + Serialize,
    {
        let key = Serializer::to_object(self.py, key)?;
        let value = Serializer::to_object(self.py, value)?;
        self.items.push(key);
        self.items.push(value);
        Ok(())
    }

    fn end(self) -> Result<PyObject> {
        PyCollect::collect(self)
    }
}

impl<'a> ser::SerializeStructVariant for MapSerializer<'_> {
    type Ok = PyObject;
    type Error = Error;

    fn serialize_field<T>(&mut self, key: &'static str, value: &T) -> Result<()>
    where
        T: ?Sized + Serialize,
    {
        ser::SerializeStruct::serialize_field(self, key, value)
    }

    fn end(self) -> Result<PyObject> {
        PyCollect::collect(self)
    }
}
