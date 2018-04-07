extern crate ignore;

#[cfg(test)]
extern crate tempdir;

mod gitignore_matcher;

pub use gitignore_matcher::GitignoreMatcher;
