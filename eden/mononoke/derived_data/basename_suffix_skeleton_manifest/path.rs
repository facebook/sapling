/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Context;
use anyhow::Result;
use mononoke_types::MPath;
use mononoke_types::MPathElement;

const SENTINEL_CHAR: u8 = b'$';

fn bsm_sentinel() -> MPathElement {
    MPathElement::new(vec![SENTINEL_CHAR]).unwrap()
}

/// Put reversed basename in beginning, plus sentinel in the end
pub(crate) struct BsmPath(MPath);

impl BsmPath {
    pub(crate) fn transform(path: MPath) -> Self {
        let (dirname, basename) = path.split_dirname();
        let mut basename = basename.clone();
        basename.reverse();
        // Let's add a sentinel add the end of the path
        // This prevents bugs, otherwise files in top-level become files
        // But they should become directories
        // So a repo with files `file` and `dir/file` will become a repo with files
        // `elif/$` and `elif/dir/$`, which otherwise could cause a file-dir conflict.
        let dirname = MPath::join_opt_element(dirname.as_ref(), &bsm_sentinel());
        Self(MPath::from(basename).join(&dirname))
    }

    pub(crate) fn from_bsm_formatted_path(path: MPath) -> Self {
        Self(path)
    }

    pub(crate) fn into_raw(self) -> MPath {
        self.0
    }

    pub(crate) fn untransform(self) -> Result<MPath> {
        let (root, rest) = self.0.split_first();
        let rest = rest.with_context(|| format!("No filename {}", root))?;
        let mut root = root.clone();
        root.reverse();
        let (dirname, basename) = rest.split_dirname();
        if basename.as_ref() != [SENTINEL_CHAR] {
            anyhow::bail!(
                "Invalid filename {}, not sentinel {}",
                basename,
                char::from(SENTINEL_CHAR),
            )
        }
        Ok(MPath::join_opt_element(dirname.as_ref(), &root))
    }
}

#[cfg(test)]
mod test {
    use quickcheck::quickcheck;

    use super::*;

    quickcheck! {
        fn roundtrip(path: MPath) -> bool {
            let bsm = BsmPath::transform(path.clone());
            bsm.untransform().unwrap() == path
        }
    }

    fn assert_transform(orig: &str, transform: &str) {
        assert_eq!(
            BsmPath::transform(MPath::new(orig).unwrap()).into_raw(),
            MPath::new(transform).unwrap()
        );
    }

    fn assert_untransform(transform: &str, orig: Option<&str>) {
        let untransform =
            BsmPath::from_bsm_formatted_path(MPath::new(transform).unwrap()).untransform();
        if let Some(orig) = orig {
            assert_eq!(untransform.unwrap(), MPath::new(orig).unwrap(),);
        } else {
            assert!(untransform.is_err());
        }
    }

    #[test]
    fn test_transform() {
        assert_transform("a/b/c", "c/a/b/$");
        assert_transform("dir/file", "elif/dir/$");
        assert_transform("file", "elif/$");
        assert_transform("dir/subdir/hello", "olleh/dir/subdir/$");
        assert_transform("eden/mononoke/main.rs", "sr.niam/eden/mononoke/$");
    }

    #[test]
    fn test_untransform() {
        assert_untransform("c/a/b/$", Some("a/b/c"));
        assert_untransform("elif/dir/$", Some("dir/file"));
        assert_untransform("elif/$", Some("file"));
        assert_untransform("olleh/dir/subdir/$", Some("dir/subdir/hello"));
        assert_untransform("sr.niam/eden/mononoke/$", Some("eden/mononoke/main.rs"));
        assert_untransform("$", None);
        assert_untransform("c/b/a", None);
    }
}
