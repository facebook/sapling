mod de;
mod error;
mod ser;

use self::de::Deserializer;
use self::ser::Serializer;
use serde::{Deserialize, Serialize};

pub use self::error::{Error, Result};

pub fn serialize<T>(out: &mut Vec<u8>, value: &T) -> Result<()>
where
    T: Serialize,
{
    let mut ser = Serializer::new(out);
    Serialize::serialize(value, &mut ser)
}

pub fn deserialize<'de, T>(bytes: &'de [u8]) -> Result<T>
where
    T: Deserialize<'de>,
{
    let mut de = Deserializer::new(bytes);
    Deserialize::deserialize(&mut de)
}
