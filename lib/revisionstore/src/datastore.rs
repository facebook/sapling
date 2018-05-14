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
    fn getdeltachain(&self, key: &Key) -> Result<Vec<Delta>>;
    fn getmeta(&self, key: &Key) -> Result<Metadata>;
    fn getmissing(&self, keys: &[Key]) -> Result<Vec<Key>>;
}
