use error::Result;
use key::Key;
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq)]
pub enum RepackOutputType {
    Data,
    History,
}

pub trait IterableStore {
    fn iter<'a>(&'a self) -> Box<Iterator<Item = Result<Key>> + 'a>;
}

pub struct RepackResult {
    packed_keys: HashSet<Key>,
    created_packs: HashSet<PathBuf>,
}

impl RepackResult {
    // new() should probably be crate-local, since the repack implementation is the only thing that
    // constructs it. But the python integration layer currently needs to construct this, so it
    // needs to be externally public for now.
    pub fn new(packed_keys: HashSet<Key>, created_packs: HashSet<PathBuf>) -> Self {
        RepackResult {
            packed_keys,
            created_packs,
        }
    }

    /// Returns the set of created pack files. The paths do not include the .pack/.idx suffixes.
    pub fn created_packs(&self) -> &HashSet<PathBuf> {
        &self.created_packs
    }

    pub fn packed_keys(&self) -> &HashSet<Key> {
        &self.packed_keys
    }
}

pub trait Repackable: IterableStore {
    fn delete(&self) -> Result<()>;
    fn id(&self) -> &Arc<PathBuf>;
    fn kind(&self) -> RepackOutputType;

    /// An iterator containing every key in the store, and identifying information for where it
    /// came from and what type it is (data vs history).
    fn repack_iter<'a>(
        &'a self,
    ) -> Box<Iterator<Item = Result<(Arc<PathBuf>, RepackOutputType, Key)>> + 'a> {
        let id = self.id().clone();
        let kind = self.kind().clone();
        Box::new(
            self.iter()
                .map(move |k| k.map(|k| (id.clone(), kind.clone(), k))),
        )
    }

    fn cleanup(&self, result: &RepackResult) -> Result<()> {
        let owned_keys = self.iter().collect::<Result<HashSet<Key>>>()?;
        if owned_keys.is_subset(result.packed_keys())
            && !result.created_packs().contains(self.id().as_ref())
        {
            self.delete()?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::SeedableRng;
    use rand::chacha::ChaChaRng;
    use std::cell::RefCell;
    use types::node::Node;

    struct FakeStore {
        pub kind: RepackOutputType,
        pub id: Arc<PathBuf>,
        pub keys: Vec<Key>,
        pub deleted: RefCell<bool>,
    }

    impl IterableStore for FakeStore {
        fn iter<'a>(&'a self) -> Box<Iterator<Item = Result<Key>> + 'a> {
            Box::new(self.keys.iter().map(|k| Ok(k.clone())))
        }
    }

    impl Repackable for FakeStore {
        fn delete(&self) -> Result<()> {
            let mut deleted = self.deleted.borrow_mut();
            *deleted = true;
            Ok(())
        }

        fn id(&self) -> &Arc<PathBuf> {
            &self.id
        }

        fn kind(&self) -> RepackOutputType {
            self.kind.clone()
        }
    }

    #[test]
    fn test_repackable() {
        let mut rng = ChaChaRng::from_seed([0u8; 32]);
        let store = FakeStore {
            kind: RepackOutputType::Data,
            id: Arc::new(PathBuf::from("foo/bar")),
            keys: vec![
                Key::new(Box::new([0]), Node::random(&mut rng)),
                Key::new(Box::new([0]), Node::random(&mut rng)),
            ],
            deleted: RefCell::new(false),
        };

        let mut marked: Vec<(Arc<PathBuf>, RepackOutputType, Key)> = vec![];
        for entry in store.repack_iter() {
            marked.push(entry.unwrap());
        }
        assert_eq!(
            marked,
            vec![
                (store.id.clone(), store.kind.clone(), store.keys[0].clone()),
                (store.id.clone(), store.kind.clone(), store.keys[1].clone()),
            ]
        );

        // Test cleanup where the packed keys don't some store keys
        let mut packed_keys = HashSet::new();
        packed_keys.insert(store.keys[0].clone());
        let mut created_packs = HashSet::new();
        store
            .cleanup(&RepackResult::new(
                packed_keys.clone(),
                created_packs.clone(),
            ))
            .unwrap();
        assert_eq!(*store.deleted.borrow(), false);

        // Test cleanup where all keys are packe but created includes this store
        packed_keys.insert(store.keys[1].clone());
        created_packs.insert(store.id().to_path_buf());
        store
            .cleanup(&RepackResult::new(
                packed_keys.clone(),
                created_packs.clone(),
            ))
            .unwrap();
        assert_eq!(*store.deleted.borrow(), false);

        // Test cleanup where all keys are packed and created doesn't include this store
        created_packs.clear();
        store
            .cleanup(&RepackResult::new(
                packed_keys.clone(),
                created_packs.clone(),
            ))
            .unwrap();
        assert_eq!(*store.deleted.borrow(), true);
    }
}
