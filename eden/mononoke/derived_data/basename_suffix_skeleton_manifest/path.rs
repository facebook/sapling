/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use mononoke_types::MPath;

/// Put reversed basename in beginning
pub(crate) struct BssmPath(MPath);

impl BssmPath {
    pub(crate) fn transform(path: MPath) -> Self {
        let mut rev_basename = path.split_dirname().1.clone();
        rev_basename.reverse();
        // Let's add the basename in the end of the path as well
        // This prevents bugs, otherwise files in top-level become files
        // But they should become directories
        // So a repo with files `file` and `dir/file` will become a repo with files
        // `elif/file` and `elif/dir/file`, which otherwise could cause a file-dir conflict.
        // It also conserves ordering of files, for the same basename.
        Self(MPath::from(rev_basename).join(&path))
    }

    #[cfg(test)]
    pub(crate) fn from_bsm_formatted_path(path: MPath) -> Self {
        Self(path)
    }

    pub(crate) fn into_raw(self) -> MPath {
        self.0
    }

    #[cfg(test)]
    pub(crate) fn untransform(self) -> anyhow::Result<MPath> {
        use anyhow::Context;

        let (rev_basename, rest) = self.0.split_first();
        let rest = rest.with_context(|| format!("Invalid format for path {}", self.0))?;
        let mut rev_basename2 = rest.split_dirname().1.clone();
        rev_basename2.reverse();
        if *rev_basename != rev_basename2 {
            anyhow::bail!(
                "Invalid format for path {}, expected reverse basename in root.",
                self.0
            )
        }
        Ok(rest)
    }
}

#[cfg(test)]
mod test {
    use quickcheck::quickcheck;

    use super::*;

    quickcheck! {
        fn roundtrip(path: MPath) -> bool {
            let bsm = BssmPath::transform(path.clone());
            bsm.untransform().unwrap() == path
        }
    }

    fn assert_transform(orig: &str, transform: &str) {
        assert_eq!(
            BssmPath::transform(MPath::new(orig).unwrap()).into_raw(),
            MPath::new(transform).unwrap()
        );
    }

    fn assert_untransform(transform: &str, orig: Option<&str>) {
        let untransform =
            BssmPath::from_bsm_formatted_path(MPath::new(transform).unwrap()).untransform();
        if let Some(orig) = orig {
            assert_eq!(untransform.unwrap(), MPath::new(orig).unwrap(),);
        } else {
            assert!(untransform.is_err());
        }
    }

    #[test]
    fn test_transform() {
        assert_transform("a/b/c", "c/a/b/c");
        assert_transform("dir/file", "elif/dir/file");
        assert_transform("file", "elif/file");
        assert_transform("dir/subdir/hello", "olleh/dir/subdir/hello");
        assert_transform("eden/mononoke/main.rs", "sr.niam/eden/mononoke/main.rs");
    }

    #[test]
    fn test_untransform() {
        assert_untransform("c/a/b/c", Some("a/b/c"));
        assert_untransform("elif/dir/file", Some("dir/file"));
        assert_untransform("elif/file", Some("file"));
        assert_untransform("olleh/dir/subdir/hello", Some("dir/subdir/hello"));
        assert_untransform(
            "sr.niam/eden/mononoke/main.rs",
            Some("eden/mononoke/main.rs"),
        );
        assert_untransform("$", None);
        assert_untransform("c/b/a", None);
    }
}
