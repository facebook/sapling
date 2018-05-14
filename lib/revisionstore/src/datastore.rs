use error::Result;
use key::Key;

pub trait DataStore {
    fn get(&self, key: &Key) -> Result<Vec<u8>>;
}
