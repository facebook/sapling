use byteorder::{ReadBytesExt, WriteBytesExt};
use std::io::{Read, Write};

use error::Result;

#[derive(Debug, Fail)]
#[fail(display = "DataIndex Error: {:?}", _0)]
struct DataIndexError(String);

#[derive(Debug, PartialEq)]
struct DataIndexOptions {
    version: u8,
    // Indicates whether to use the large fanout (2 bytes) or the small (1 byte)
    large: bool,
}

impl DataIndexOptions {
    pub fn read<T: Read>(reader: &mut T) -> Result<DataIndexOptions> {
        let version = reader.read_u8()?;
        if version > 1 {
            return Err(DataIndexError(format!("unsupported version '{:?}'", version)).into());
        };

        let raw_config = reader.read_u8()?;
        let large = match raw_config {
            0b10000000 => true,
            0 => false,
            _ => {
                return Err(DataIndexError(format!("invalid data index '{:?}'", raw_config)).into())
            }
        };
        Ok(DataIndexOptions { version, large })
    }

    pub fn write<T: Write>(&self, writer: &mut T) -> Result<()> {
        writer.write_u8(self.version)?;
        writer.write_u8(if self.large { 0b10000000 } else { 0 })?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn test_header_invalid() {
        let buf: Vec<u8> = vec![2, 0];
        DataIndexOptions::read(&mut Cursor::new(buf)).expect_err("invalid read");

        let buf: Vec<u8> = vec![0, 1];
        DataIndexOptions::read(&mut Cursor::new(buf)).expect_err("invalid read");
    }

    quickcheck! {
        fn test_header_serialization(version: u8, large: bool) -> bool {
            let version = version % 2;
            let options = DataIndexOptions { version, large };
            let mut buf: Vec<u8> = vec![];
            options.write(&mut buf).expect("write");
            let parsed_options = DataIndexOptions::read(&mut Cursor::new(buf)).expect("read");
            options == parsed_options
        }
    }
}
