// Copyright (c) Facebook, Inc. and its affiliates.
// Copyright (c) David Tolnay <dtolnay@gmail.com>
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2. Copyright Facebook, Inc.

use crate::{Error, Result};
use byteorder::{NetworkEndian, WriteBytesExt};
use serde;
use serde::ser::{
    SerializeMap, SerializeSeq, SerializeStruct, SerializeStructVariant, SerializeTuple,
    SerializeTupleStruct, SerializeTupleVariant,
};
use std::io::Write;

pub struct Serializer<W> {
    writer: W,
}

impl<W> Serializer<W>
where
    W: Write,
{
    pub fn new(w: W) -> Self {
        Serializer { writer: w }
    }
}

impl<'a, W> serde::Serializer for &'a mut Serializer<W>
where
    W: Write,
{
    type Ok = ();
    type Error = Error;

    type SerializeSeq = Self;
    type SerializeTuple = Self;
    type SerializeTupleStruct = Self;
    type SerializeTupleVariant = Self;
    type SerializeMap = Self;
    type SerializeStruct = Self;
    type SerializeStructVariant = Self;

    #[inline]
    fn serialize_unit(self) -> Result<()> {
        Ok(())
    }

    #[inline]
    fn serialize_unit_struct(self, _: &'static str) -> Result<()> {
        Ok(())
    }

    #[inline]
    fn serialize_bool(self, v: bool) -> Result<()> {
        self.writer.write_u8(v as u8).map_err(From::from)
    }

    #[inline]
    fn serialize_u8(self, v: u8) -> Result<()> {
        self.writer.write_u8(v).map_err(From::from)
    }

    #[inline]
    fn serialize_u16(self, v: u16) -> Result<()> {
        self.writer
            .write_u16::<NetworkEndian>(v)
            .map_err(From::from)
    }

    #[inline]
    fn serialize_u32(self, v: u32) -> Result<()> {
        self.writer
            .write_u32::<NetworkEndian>(v)
            .map_err(From::from)
    }

    #[inline]
    fn serialize_u64(self, v: u64) -> Result<()> {
        self.writer
            .write_u64::<NetworkEndian>(v)
            .map_err(From::from)
    }

    #[inline]
    fn serialize_i8(self, v: i8) -> Result<()> {
        self.writer.write_i8(v).map_err(From::from)
    }

    #[inline]
    fn serialize_i16(self, v: i16) -> Result<()> {
        self.writer
            .write_i16::<NetworkEndian>(v)
            .map_err(From::from)
    }

    #[inline]
    fn serialize_i32(self, v: i32) -> Result<()> {
        self.writer
            .write_i32::<NetworkEndian>(v)
            .map_err(From::from)
    }

    #[inline]
    fn serialize_i64(self, v: i64) -> Result<()> {
        self.writer
            .write_i64::<NetworkEndian>(v)
            .map_err(From::from)
    }

    #[inline]
    fn serialize_f32(self, v: f32) -> Result<()> {
        self.writer
            .write_f32::<NetworkEndian>(v)
            .map_err(From::from)
    }

    #[inline]
    fn serialize_f64(self, v: f64) -> Result<()> {
        self.writer
            .write_f64::<NetworkEndian>(v)
            .map_err(From::from)
    }

    #[inline]
    fn serialize_str(self, v: &str) -> Result<()> {
        self.serialize_u64(v.len() as u64)?;
        self.writer.write_all(v.as_bytes()).map_err(From::from)
    }

    #[inline]
    fn serialize_char(self, c: char) -> Result<()> {
        self.writer
            .write_all(encode_utf8(c).as_slice())
            .map_err(From::from)
    }

    #[inline]
    fn serialize_bytes(self, v: &[u8]) -> Result<()> {
        self.serialize_u64(v.len() as u64)?;
        self.writer.write_all(v).map_err(From::from)
    }

    #[inline]
    fn serialize_none(self) -> Result<()> {
        self.writer.write_u8(0).map_err(From::from)
    }

    #[inline]
    fn serialize_some<T: ?Sized>(self, v: &T) -> Result<()>
    where
        T: serde::Serialize,
    {
        self.writer.write_u8(1)?;
        v.serialize(self)
    }

    #[inline]
    fn serialize_seq(self, len: Option<usize>) -> Result<Self::SerializeSeq> {
        let len = len.expect("do not know how to serialize a sequence with no length");
        self.serialize_u64(len as u64)?;
        Ok(self)
    }

    #[inline]
    fn serialize_tuple(self, _len: usize) -> Result<Self::SerializeTuple> {
        Ok(self)
    }

    #[inline]
    fn serialize_tuple_struct(
        self,
        _name: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeTupleStruct> {
        Ok(self)
    }

    #[inline]
    fn serialize_tuple_variant(
        self,
        _name: &'static str,
        variant_index: u32,
        _variant: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeTupleVariant> {
        self.serialize_u32(variant_index)?;
        Ok(self)
    }

    #[inline]
    fn serialize_map(self, len: Option<usize>) -> Result<Self::SerializeMap> {
        let len = len.expect("do not know how to serialize a map with no length");
        self.serialize_u64(len as u64)?;
        Ok(self)
    }

    #[inline]
    fn serialize_struct(self, _name: &'static str, _len: usize) -> Result<Self::SerializeStruct> {
        Ok(self)
    }

    #[inline]
    fn serialize_struct_variant(
        self,
        _name: &'static str,
        variant_index: u32,
        _variant: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeStructVariant> {
        self.serialize_u32(variant_index)?;
        Ok(self)
    }

    #[inline]
    fn serialize_newtype_struct<T: ?Sized>(self, _name: &'static str, value: &T) -> Result<()>
    where
        T: serde::ser::Serialize,
    {
        value.serialize(self)
    }

    #[inline]
    fn serialize_newtype_variant<T: ?Sized>(
        self,
        _name: &'static str,
        variant_index: u32,
        _variant: &'static str,
        value: &T,
    ) -> Result<()>
    where
        T: serde::ser::Serialize,
    {
        self.serialize_u32(variant_index)?;
        value.serialize(self)
    }

    #[inline]
    fn serialize_unit_variant(
        self,
        _name: &'static str,
        variant_index: u32,
        _variant: &'static str,
    ) -> Result<()> {
        self.serialize_u32(variant_index)
    }
}

impl<'a, W> SerializeSeq for &'a mut Serializer<W>
where
    W: Write,
{
    type Ok = ();
    type Error = Error;

    #[inline]
    fn serialize_element<V: ?Sized>(&mut self, value: &V) -> Result<()>
    where
        V: serde::Serialize,
    {
        value.serialize(&mut **self)
    }

    #[inline]
    fn end(self) -> Result<()> {
        Ok(())
    }
}

impl<'a, W> SerializeTuple for &'a mut Serializer<W>
where
    W: Write,
{
    type Ok = ();
    type Error = Error;

    #[inline]
    fn serialize_element<V: ?Sized>(&mut self, value: &V) -> Result<()>
    where
        V: serde::Serialize,
    {
        value.serialize(&mut **self)
    }

    #[inline]
    fn end(self) -> Result<()> {
        Ok(())
    }
}

impl<'a, W> SerializeTupleStruct for &'a mut Serializer<W>
where
    W: Write,
{
    type Ok = ();
    type Error = Error;

    #[inline]
    fn serialize_field<V: ?Sized>(&mut self, value: &V) -> Result<()>
    where
        V: serde::Serialize,
    {
        value.serialize(&mut **self)
    }

    #[inline]
    fn end(self) -> Result<()> {
        Ok(())
    }
}

impl<'a, W> SerializeTupleVariant for &'a mut Serializer<W>
where
    W: Write,
{
    type Ok = ();
    type Error = Error;

    #[inline]
    fn serialize_field<V: ?Sized>(&mut self, value: &V) -> Result<()>
    where
        V: serde::Serialize,
    {
        value.serialize(&mut **self)
    }

    #[inline]
    fn end(self) -> Result<()> {
        Ok(())
    }
}

impl<'a, W> SerializeMap for &'a mut Serializer<W>
where
    W: Write,
{
    type Ok = ();
    type Error = Error;

    #[inline]
    fn serialize_key<K: ?Sized>(&mut self, key: &K) -> Result<()>
    where
        K: serde::Serialize,
    {
        key.serialize(&mut **self)
    }

    #[inline]
    fn serialize_value<V: ?Sized>(&mut self, value: &V) -> Result<()>
    where
        V: serde::Serialize,
    {
        value.serialize(&mut **self)
    }

    #[inline]
    fn end(self) -> Result<()> {
        Ok(())
    }
}

impl<'a, W> SerializeStruct for &'a mut Serializer<W>
where
    W: Write,
{
    type Ok = ();
    type Error = Error;

    #[inline]
    fn serialize_field<V: ?Sized>(&mut self, _key: &'static str, value: &V) -> Result<()>
    where
        V: serde::Serialize,
    {
        value.serialize(&mut **self)
    }

    #[inline]
    fn end(self) -> Result<()> {
        Ok(())
    }
}

impl<'a, W> SerializeStructVariant for &'a mut Serializer<W>
where
    W: Write,
{
    type Ok = ();
    type Error = Error;

    #[inline]
    fn serialize_field<V: ?Sized>(&mut self, _key: &'static str, value: &V) -> Result<()>
    where
        V: serde::Serialize,
    {
        value.serialize(&mut **self)
    }

    #[inline]
    fn end(self) -> Result<()> {
        Ok(())
    }
}

#[inline]
fn encode_utf8(c: char) -> EncodeUtf8 {
    const TAG_CONT: u8 = 0b1000_0000;
    const TAG_TWO_B: u8 = 0b1100_0000;
    const TAG_THREE_B: u8 = 0b1110_0000;
    const TAG_FOUR_B: u8 = 0b1111_0000;
    const MAX_ONE_B: u32 = 0x80;
    const MAX_TWO_B: u32 = 0x800;
    const MAX_THREE_B: u32 = 0x10000;

    let code = c as u32;
    let mut buf = [0; 4];
    let pos = if code < MAX_ONE_B {
        buf[3] = code as u8;
        3
    } else if code < MAX_TWO_B {
        buf[2] = (code >> 6 & 0x1F) as u8 | TAG_TWO_B;
        buf[3] = (code & 0x3F) as u8 | TAG_CONT;
        2
    } else if code < MAX_THREE_B {
        buf[1] = (code >> 12 & 0x0F) as u8 | TAG_THREE_B;
        buf[2] = (code >> 6 & 0x3F) as u8 | TAG_CONT;
        buf[3] = (code & 0x3F) as u8 | TAG_CONT;
        1
    } else {
        buf[0] = (code >> 18 & 0x07) as u8 | TAG_FOUR_B;
        buf[1] = (code >> 12 & 0x3F) as u8 | TAG_CONT;
        buf[2] = (code >> 6 & 0x3F) as u8 | TAG_CONT;
        buf[3] = (code & 0x3F) as u8 | TAG_CONT;
        0
    };
    EncodeUtf8 { buf, pos }
}

struct EncodeUtf8 {
    buf: [u8; 4],
    pos: usize,
}

impl EncodeUtf8 {
    #[inline]
    fn as_slice(&self) -> &[u8] {
        &self.buf[self.pos..]
    }
}
