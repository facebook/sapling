/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use mononoke_types::path::MPath;
use mononoke_types::MPathElement;
use mononoke_types::NonRootMPath;
use mononoke_types::TrieMap;

#[derive(Clone, Debug)]
pub struct PathTree<V> {
    pub value: V,
    pub subentries: TrieMap<Self>,
}

impl<V> PathTree<V> {
    pub fn deconstruct(self) -> (V, Vec<(MPathElement, Self)>) {
        (
            self.value,
            self.subentries
                .into_iter()
                .map(|(path, subtree)| {
                    (
                        MPathElement::from_smallvec(path)
                            .expect("Only MPaths are inserted into PathTree"),
                        subtree,
                    )
                })
                .collect(),
        )
    }

    pub fn get(&self, path: &MPath) -> Option<&V> {
        let mut tree = self;
        for elem in path {
            match tree.subentries.get(elem.as_ref()) {
                Some(subtree) => tree = subtree,
                None => return None,
            }
        }
        Some(&tree.value)
    }
}

impl<V> PathTree<V>
where
    V: Default,
{
    pub fn insert(&mut self, path: MPath, value: V) {
        let node = path.into_iter().fold(self, |node, element| {
            node.subentries.get_or_insert_default(element)
        });
        node.value = value;
    }

    pub fn insert_and_merge<T>(&mut self, path: MPath, value: T)
    where
        V: Extend<T>,
    {
        let node = path.into_iter().fold(self, |node, element| {
            node.subentries.get_or_insert_default(element)
        });
        node.value.extend(std::iter::once(value));
    }

    pub fn insert_and_prune(&mut self, path: MPath, value: V) {
        let node = path.into_iter().fold(self, |node, element| {
            node.subentries.get_or_insert_default(element)
        });
        node.value = value;
        node.subentries.clear();
    }
}

impl<V> Default for PathTree<V>
where
    V: Default,
{
    fn default() -> Self {
        Self {
            value: Default::default(),
            subentries: Default::default(),
        }
    }
}

impl<V> FromIterator<(MPath, V)> for PathTree<V>
where
    V: Default,
{
    fn from_iter<I>(iter: I) -> Self
    where
        I: IntoIterator<Item = (MPath, V)>,
    {
        let mut tree: Self = Default::default();
        for (path, value) in iter {
            tree.insert(path, value);
        }
        tree
    }
}

impl<V> FromIterator<(NonRootMPath, V)> for PathTree<V>
where
    V: Default,
{
    fn from_iter<I>(iter: I) -> Self
    where
        I: IntoIterator<Item = (NonRootMPath, V)>,
    {
        let mut tree: Self = Default::default();
        for (path, value) in iter {
            tree.insert(MPath::from(path), value);
        }
        tree
    }
}

pub struct PathTreeIter<V> {
    frames: Vec<(MPath, PathTree<V>)>,
}

impl<V> Iterator for PathTreeIter<V> {
    type Item = (MPath, V);

    fn next(&mut self) -> Option<Self::Item> {
        let (path, path_tree) = self.frames.pop()?;
        let (value, subentries) = path_tree.deconstruct();

        for (name, subentry) in subentries {
            self.frames.push((path.join(&name), subentry));
        }
        Some((path, value))
    }
}

impl<V> IntoIterator for PathTree<V> {
    type Item = (MPath, V);
    type IntoIter = PathTreeIter<V>;

    fn into_iter(self) -> Self::IntoIter {
        PathTreeIter {
            frames: vec![(MPath::ROOT, self)],
        }
    }
}

#[cfg(test)]
mod tests {
    use anyhow::Result;
    use mononoke_macros::mononoke;

    use super::*;

    #[mononoke::test]
    fn test_path_tree() -> Result<()> {
        let tree = PathTree::from_iter(vec![
            (MPath::new("/one/two/three")?, true),
            (MPath::new("/one/two/four")?, true),
            (MPath::new("/one/two")?, true),
            (MPath::new("/five")?, true),
        ]);

        let reference = vec![
            (MPath::ROOT, false),
            (MPath::new("one")?, false),
            (MPath::new("one/two")?, true),
            (MPath::new("one/two/three")?, true),
            (MPath::new("one/two/four")?, true),
            (MPath::new("five")?, true),
        ];

        assert_eq!(Vec::from_iter(tree), reference);
        Ok(())
    }

    #[mononoke::test]
    fn test_path_insert_and_merge() -> Result<()> {
        let mut tree = PathTree::<Vec<_>>::default();
        let items = vec![
            (MPath::new("/one/two/three")?, true),
            (MPath::new("/one/two/three")?, false),
        ];
        for (path, value) in items {
            tree.insert_and_merge(path, value);
        }

        let reference = vec![
            (MPath::ROOT, vec![]),
            (MPath::new("one")?, vec![]),
            (MPath::new("one/two")?, vec![]),
            (MPath::new("one/two/three")?, vec![true, false]),
        ];

        assert_eq!(Vec::from_iter(tree), reference);
        Ok(())
    }
}
