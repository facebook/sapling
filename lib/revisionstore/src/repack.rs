use error::Result;
use key::Key;

pub trait IterableStore {
    fn iter<'a>(&'a self) -> Box<Iterator<Item = Result<Key>> + 'a>;
}
