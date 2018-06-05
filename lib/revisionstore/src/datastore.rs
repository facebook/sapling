use error::Result;
use key::Key;

pub struct Delta {
    pub data: Box<[u8]>,
    pub base: Key,
    pub key: Key,
}

pub struct Metadata {
    pub size: Option<u64>,
    pub flags: Option<u16>,
}

pub trait DataStore {
    fn get(&self, key: &Key) -> Result<Vec<u8>>;
    fn get_delta_chain(&self, key: &Key) -> Result<Vec<Delta>>;
    fn get_meta(&self, key: &Key) -> Result<Metadata>;
    fn get_missing(&self, keys: &[Key]) -> Result<Vec<Key>>;
}
