extern crate ignore;
extern crate lru_cache;

#[cfg(test)]
extern crate tempdir;

mod gitignore_matcher;

pub use gitignore_matcher::GitignoreMatcher;
