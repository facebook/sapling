/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::marker::PhantomData;

use cpython::PyBytes;
use cpython::PyDict;
use cpython::PyObject;
use cpython::PyResult;
use cpython::PyTuple;
use cpython::Python;
use cpython::PythonObject;
use cpython::ToPyObject;
use serde::ser;
use serde::Serialize;

use crate::PyErr as Error;

/// Serialize into Python object.
pub fn to_object<T>(py: Python, value: &T) -> PyResult<PyObject>
where
    T: Serialize,
{
    let serializer = Serializer { py };
    value.serialize(&serializer).map_err(Into::into)
}

// ------- Serializer Types -------

type Result<T> = std::result::Result<T, Error>;

struct Serializer<'a> {
    py: Python<'a>,
}

trait PyCollectItems {
    fn collect_items(py: Python, items: &[PyObject]) -> Result<PyObject>;
}

trait PyBuildVariant {
    fn build_variant(&self, py: Python, obj: PyObject) -> Result<PyObject>;
}

struct PyItems<'a, C, V> {
    py: Python<'a>,
    items: Vec<PyObject>,
    collector: PhantomData<C>,
    variant: V,
}

impl<'a, C, V> PyItems<'a, C, V> {
    fn new(py: Python<'a>, variant: V) -> Self {
        Self {
            py,
            items: Default::default(),
            collector: PhantomData,
            variant,
        }
    }
}

impl<'a, C, V> PyItems<'a, C, V>
where
    C: PyCollectItems,
    V: PyBuildVariant,
{
    fn collect(&self) -> Result<PyObject> {
        let obj = C::collect_items(self.py, &self.items)?;
        self.variant.build_variant(self.py, obj)
    }
}

impl PyBuildVariant for &'static str {
    fn build_variant(&self, py: Python, obj: PyObject) -> Result<PyObject> {
        enum_variant(py, self, obj)
    }
}

impl PyBuildVariant for () {
    fn build_variant(&self, _py: Python, obj: PyObject) -> Result<PyObject> {
        Ok(obj)
    }
}

struct BuildList;
struct BuildTuple;
struct BuildDict;

impl PyCollectItems for BuildList {
    fn collect_items(py: Python, items: &[PyObject]) -> Result<PyObject> {
        Ok(items.to_py_object(py).into_object())
    }
}

impl PyCollectItems for BuildTuple {
    fn collect_items(py: Python, items: &[PyObject]) -> Result<PyObject> {
        Ok(PyTuple::new(py, items).into_object())
    }
}

impl PyCollectItems for BuildDict {
    fn collect_items(py: Python, items: &[PyObject]) -> Result<PyObject> {
        let dict = PyDict::new(py);
        for chunk in items.chunks(2) {
            if let [key, value] = chunk {
                dict.set_item(py, key, value)?;
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

    type SerializeSeq = PyItems<'b, BuildList, ()>;
    type SerializeTuple = PyItems<'b, BuildTuple, ()>;
    type SerializeTupleStruct = PyItems<'b, BuildTuple, ()>;
    type SerializeTupleVariant = PyItems<'b, BuildTuple, &'static str>;
    type SerializeMap = PyItems<'b, BuildDict, ()>;
    type SerializeStruct = PyItems<'b, BuildDict, ()>;
    type SerializeStructVariant = PyItems<'b, BuildDict, &'static str>;

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
        Ok(v.to_py_object(self.py).into_object())
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
        let value = value.serialize(self)?;
        enum_variant(self.py, variant, value)
    }

    fn serialize_seq(self, _len: Option<usize>) -> Result<Self::SerializeSeq> {
        Ok(Self::SerializeSeq::new(self.py, ()))
    }

    fn serialize_tuple(self, _len: usize) -> Result<Self::SerializeTuple> {
        Ok(Self::SerializeTuple::new(self.py, ()))
    }

    fn serialize_tuple_struct(
        self,
        _name: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeTupleStruct> {
        Ok(Self::SerializeTupleStruct::new(self.py, ()))
    }

    fn serialize_tuple_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        variant: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeTupleVariant> {
        Ok(Self::SerializeTupleVariant::new(self.py, variant))
    }

    fn serialize_map(self, _len: Option<usize>) -> Result<Self::SerializeMap> {
        Ok(Self::SerializeMap::new(self.py, ()))
    }

    fn serialize_struct(self, _name: &'static str, _len: usize) -> Result<Self::SerializeStruct> {
        Ok(Self::SerializeStruct::new(self.py, ()))
    }

    fn serialize_struct_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        variant: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeStructVariant> {
        Ok(Self::SerializeStructVariant::new(self.py, variant))
    }
}

macro_rules! impl_seq {
    ($trait: ty,  $name: tt) => {
        impl<C, V> $trait for PyItems<'_, C, V>
        where
            C: PyCollectItems,
            V: PyBuildVariant,
        {
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
                self.collect()
            }
        }
    };
}

impl_seq!(ser::SerializeSeq, serialize_element);
impl_seq!(ser::SerializeTuple, serialize_element);
impl_seq!(ser::SerializeTupleStruct, serialize_field);
impl_seq!(ser::SerializeTupleVariant, serialize_field);

impl<V> ser::SerializeMap for PyItems<'_, BuildDict, V>
where
    V: PyBuildVariant,
{
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
        self.collect()
    }
}

impl<V> ser::SerializeStruct for PyItems<'_, BuildDict, V>
where
    V: PyBuildVariant,
{
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
        self.collect()
    }
}

impl<V> ser::SerializeStructVariant for PyItems<'_, BuildDict, V>
where
    V: PyBuildVariant,
{
    type Ok = PyObject;
    type Error = Error;

    fn serialize_field<T>(&mut self, key: &'static str, value: &T) -> Result<()>
    where
        T: ?Sized + Serialize,
    {
        ser::SerializeStruct::serialize_field(self, key, value)
    }

    fn end(self) -> Result<PyObject> {
        self.collect()
    }
}

// ------- Utilities -------

/// Convert `value` to `{key: value}` to represent an enum variant.
fn enum_variant(py: Python, key: &'static str, value: PyObject) -> Result<PyObject> {
    let dict = PyDict::new(py);
    dict.set_item(py, key, value)?;
    Ok(dict.into_object())
}
