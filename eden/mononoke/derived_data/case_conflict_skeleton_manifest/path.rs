/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#[cfg(test)]
use itertools::Itertools;
use mononoke_types::MPathElement;
use mononoke_types::NonRootMPath;

#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct CcsmPath(NonRootMPath);

impl CcsmPath {
    pub fn transform(path: NonRootMPath) -> Option<Self> {
        Some(Self(
            NonRootMPath::try_from(
                path.into_iter()
                    .map(|element| {
                        Some(vec![
                            MPathElement::new_from_slice(element.to_lowercase_utf8()?.as_ref())
                                .ok()?,
                            element,
                        ])
                    })
                    .collect::<Option<Vec<_>>>()?
                    .into_iter()
                    .flatten()
                    .collect::<Vec<_>>(),
            )
            .ok()?,
        ))
    }

    #[cfg(test)]
    pub(crate) fn from_non_root_mpath(path: NonRootMPath) -> Self {
        Self(path)
    }

    pub fn into_raw(self) -> NonRootMPath {
        self.0
    }

    #[cfg(test)]
    pub(crate) fn untransform(self) -> anyhow::Result<NonRootMPath> {
        if self.0.num_components() % 2 != 0 {
            anyhow::bail!("Ccsm transformed path doesn't have even length: {}", self.0);
        }

        let mut path_elements = vec![];
        for (lowercase, original) in self.0.iter().tuples() {
            if lowercase.as_ref()
                != original
                    .to_lowercase_utf8()
                    .ok_or_else(|| {
                        anyhow::anyhow!("Can't untransform non-utf8 path element: {}", original)
                    })?
                    .as_bytes()
            {
                anyhow::bail!(
                    "Found consecutive non-matching path elements in ccsm transformed path: lowercase: {}, original: {}",
                    lowercase,
                    original,
                );
            }
            path_elements.push(original.clone());
        }
        NonRootMPath::try_from(path_elements)
    }
}

#[cfg(test)]
mod test {
    use mononoke_macros::mononoke;
    use quickcheck::quickcheck;

    use super::*;

    quickcheck! {
        fn roundtrip(path: NonRootMPath) -> bool {
            if let Some(ccsm) = CcsmPath::transform(path.clone()) {
                ccsm.untransform().unwrap() == path
            } else {
                true
            }
        }
    }

    fn assert_transform(orig: &str, transform: &str) {
        assert_eq!(
            CcsmPath::transform(NonRootMPath::new(orig).unwrap())
                .unwrap()
                .into_raw(),
            NonRootMPath::new(transform).unwrap()
        );
    }

    fn assert_untransform(transform: &str, orig: Option<&str>) {
        let untransform =
            CcsmPath::from_non_root_mpath(NonRootMPath::new(transform).unwrap()).untransform();
        if let Some(orig) = orig {
            assert_eq!(untransform.unwrap(), NonRootMPath::new(orig).unwrap(),);
        } else {
            assert!(untransform.is_err());
        }
    }

    #[mononoke::test]
    fn test_transform() {
        assert_transform("a/B/c", "a/a/b/B/c/c");
        assert_transform("diR/FiLe", "dir/diR/file/FiLe");
        assert_transform("file", "file/file");
        assert_transform("DIR/suBDir/hello", "dir/DIR/subdir/suBDir/hello/hello");
        assert_transform(
            "eden/mononoke/main.rs",
            "eden/eden/mononoke/mononoke/main.rs/main.rs",
        );
        assert_transform(
            "EDEN/mononoke/MAIN.RS",
            "eden/EDEN/mononoke/mononoke/main.rs/MAIN.RS",
        );
    }

    #[mononoke::test]
    fn test_untransform() {
        assert_untransform("a/a/b/B/c/c", Some("a/B/c"));
        assert_untransform("dir/diR/file/FiLe", Some("diR/FiLe"));
        assert_untransform("file/file", Some("file"));
        assert_untransform(
            "dir/DIR/subdir/suBDir/hello/hello",
            Some("DIR/suBDir/hello"),
        );
        assert_untransform(
            "eden/eden/mononoke/mononoke/main.rs/main.rs",
            Some("eden/mononoke/main.rs"),
        );
        assert_untransform(
            "eden/EDEN/mononoke/mononoke/main.rs/MAIN.RS",
            Some("EDEN/mononoke/MAIN.RS"),
        );

        assert_untransform("eden/mononoke/mononoke/main.rs/main.rs", None);
        assert_untransform("a/a/b/B", Some("a/B"));
        assert_untransform("a/a/b/C", None);
    }
}
