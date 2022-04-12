/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::io;
use std::io::BufRead;
use std::io::BufReader;

use futures::future::FutureExt;
use futures::future::LocalBoxFuture;
use futures::Future;

#[derive(Default, Debug)]
pub struct Profile {
    // Where this profile came from (typically a file path).
    source: String,

    // [include], [exclude] and %include
    entries: Vec<ProfileEntry>,

    // [metadata]
    title: Option<String>,
    description: Option<String>,
    hidden: Option<String>,
    version: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
enum Pattern {
    Include(String),
    Exclude(String),
}

#[derive(Debug)]
enum ProfileEntry {
    Pattern(Pattern),
    Profile(String),
}

#[derive(PartialEq)]
enum SectionType {
    Include,
    Exclude,
    Metadata,
}

impl Pattern {
    fn as_str(&self) -> &str {
        match self {
            Self::Include(p) => p,
            Self::Exclude(p) => p,
        }
    }
}

impl SectionType {
    fn from_str(value: &str) -> Option<Self> {
        match value {
            "[include]" => Some(SectionType::Include),
            "[exclude]" => Some(SectionType::Exclude),
            "[metadata]" => Some(SectionType::Metadata),
            _ => None,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Io(#[from] io::Error),

    #[error("import cycle involving {0}")]
    ImportCycle(String),

    #[error(transparent)]
    Fetch(#[from] anyhow::Error),

    #[error("unsuppported pattern type {0}")]
    UnsupportedPattern(String),
}

impl Profile {
    pub fn from_bytes(data: impl AsRef<[u8]>, source: String) -> Result<Self, io::Error> {
        let mut prof: Profile = Default::default();
        let mut current_metadata_val: Option<&mut String> = None;
        let mut section_type = SectionType::Include;

        for (mut line_num, line) in BufReader::new(data.as_ref()).lines().enumerate() {
            line_num += 1;

            let line = line?;
            let trimmed = line.trim();

            // Ingore comments and empty lines.
            if matches!(trimmed.chars().next(), Some('#' | ';') | None) {
                continue;
            }

            if let Some(p) = trimmed.strip_prefix("%include ") {
                prof.entries
                    .push(ProfileEntry::Profile(p.trim().to_string()));
            } else if let Some(section_start) = SectionType::from_str(trimmed) {
                section_type = section_start;
                current_metadata_val = None;
            } else if section_type == SectionType::Metadata {
                if line.starts_with(&[' ', '\t']) {
                    // Continuation of multiline value.
                    if let Some(ref mut val) = current_metadata_val {
                        val.push('\n');
                        val.push_str(trimmed);
                    } else {
                        tracing::warn!(%line, %source, line_num, "orphan metadata line");
                    }
                } else {
                    current_metadata_val = None;
                    if let Some((key, val)) = trimmed.split_once(&['=', ':']) {
                        let prof_val = match key.trim() {
                            "description" => &mut prof.description,
                            "title" => &mut prof.title,
                            "hidden" => &mut prof.hidden,
                            "version" => &mut prof.version,
                            _ => {
                                tracing::warn!(%line, %source, line_num, "ignoring uninteresting metadata key");
                                continue;
                            }
                        };

                        current_metadata_val = Some(prof_val.insert(val.trim().to_string()));
                    }
                }
            } else {
                if trimmed.starts_with('/') {
                    tracing::warn!(%line, %source, line_num, "ignoring sparse rule starting with /");
                    continue;
                }

                if section_type == SectionType::Include {
                    prof.entries
                        .push(ProfileEntry::Pattern(Pattern::Include(trimmed.to_string())));
                } else {
                    prof.entries
                        .push(ProfileEntry::Pattern(Pattern::Exclude(trimmed.to_string())));
                }
            }
        }

        prof.source = source;

        Ok(prof)
    }

    // Recursively flatten this profile into a DFS ordered list of rules.
    // %import statements are resolved by fetching the imported profile's
    // contents using the fetch callback. Returns a vec of each Pattern paired
    // with a String describing its provenance.
    async fn rules<B: Future<Output = anyhow::Result<Vec<u8>>>>(
        &mut self,
        mut fetch: impl FnMut(String) -> B,
    ) -> Result<Vec<(Pattern, String)>, Error> {
        fn rules_inner<'a, B: Future<Output = anyhow::Result<Vec<u8>>>>(
            prof: &'a mut Profile,
            fetch: &'a mut dyn FnMut(String) -> B,
            rules: &'a mut Vec<(Pattern, String)>,
            source: Option<&'a str>,
            // path => (contents, in_progress)
            seen: &'a mut HashMap<String, (Vec<u8>, bool)>,
        ) -> LocalBoxFuture<'a, Result<(), Error>> {
            async move {
                let source = match source {
                    Some(history) => format!("{} -> {}", history, prof.source),
                    None => prof.source.clone(),
                };

                for entry in prof.entries.iter() {
                    match entry {
                        ProfileEntry::Pattern(p) => rules.push((p.clone(), source.clone())),
                        ProfileEntry::Profile(child_path) => {
                            let entry = seen.entry(child_path.clone());
                            let data = match entry {
                                Entry::Occupied(e) => match e.into_mut() {
                                    (_, true) => {
                                        return Err(Error::ImportCycle(child_path.clone()));
                                    }
                                    (data, false) => data,
                                },
                                Entry::Vacant(e) => {
                                    let data = fetch(child_path.clone()).await?;
                                    &e.insert((data, true)).0
                                }
                            };

                            let mut child = Profile::from_bytes(&data, child_path.clone())?;
                            rules_inner(&mut child, fetch, rules, Some(&source), seen).await?;

                            if let Some((_, in_progress)) = seen.get_mut(child_path) {
                                *in_progress = false;
                            }
                        }
                    }
                }

                Ok(())
            }
            .boxed_local()
        }

        let mut rules = Vec::new();
        rules_inner(self, &mut fetch, &mut rules, None, &mut HashMap::new()).await?;
        Ok(rules)
    }
}

static ALL_PATTERN_KINDS: &[&str] = &[
    "re",
    "glob",
    "path",
    "relglob",
    "relpath",
    "relre",
    "listfile",
    "listfile0",
    "set",
    "include",
    "subinclude",
    "rootfilesin",
];

// Convert a sparse profile pattern into what the tree matcher
// expects. We only support "glob" and "path" pattern types.
fn sparse_pat_to_matcher_rule(pat: Pattern) -> Result<Vec<String>, Error> {
    static DEFAULT_TYPE: &str = "glob";

    let (pat_type, pat_text) = match pat.as_str().split_once(':') {
        Some((t, p)) => match t {
            "glob" | "path" => (t, p),
            _ => {
                if ALL_PATTERN_KINDS.contains(&t) {
                    return Err(Error::UnsupportedPattern(t.to_string()));
                } else {
                    (DEFAULT_TYPE, pat.as_str())
                }
            }
        },
        None => (DEFAULT_TYPE, pat.as_str()),
    };

    let pats = match pat_type {
        "glob" => pathmatcher::expand_curly_brackets(pat_text)
            .iter()
            .map(|s| pathmatcher::normalize_glob(s))
            .collect(),
        "path" => vec![pathmatcher::plain_to_glob(pat_text)],
        _ => unreachable!(),
    };

    let make_recursive = |p: String| -> String {
        if p.is_empty() || p.ends_with('/') {
            p + "**"
        } else {
            p + "/**"
        }
    };

    Ok(pats
        .into_iter()
        // Adjust glob to ensure sparse rules match everything below them.
        .map(make_recursive)
        .map(|p| match pat {
            Pattern::Exclude(_) => format!("!{}", p),
            Pattern::Include(_) => p,
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use anyhow::anyhow;

    use super::*;

    // Returns a profile's (includes, excludes, profiles).
    fn split_prof(prof: &Profile) -> (Vec<&str>, Vec<&str>, Vec<&str>) {
        let (mut inc, mut exc, mut profs) = (vec![], vec![], vec![]);
        for entry in &prof.entries {
            match entry {
                ProfileEntry::Pattern(Pattern::Include(p)) => inc.push(p.as_ref()),
                ProfileEntry::Pattern(Pattern::Exclude(p)) => exc.push(p.as_ref()),
                ProfileEntry::Profile(p) => profs.push(p.as_ref()),
            }
        }
        (inc, exc, profs)
    }

    #[test]
    fn test_parsing() {
        let got = Profile::from_bytes(
            b"
; hello
  # there

a
[metadata]
boring = banana
title  =   foo
[include]
glob:b/**/z
/skip/me
%include  other.sparse
 [exclude]
c
/skip/me

[metadata]
	skip me
description:howdy
 doody
version : 123
hidden=your eyes
	only

",
            "test".to_string(),
        )
        .unwrap();

        assert_eq!(got.source, "test");

        let (inc, exc, profs) = split_prof(&got);
        assert_eq!(inc, vec!["a", "glob:b/**/z"]);
        assert_eq!(exc, vec!["c"]);
        assert_eq!(profs, vec!["other.sparse"]);

        assert_eq!(got.title.unwrap(), "foo");
        assert_eq!(got.description.unwrap(), "howdy\ndoody");
        assert_eq!(got.hidden.unwrap(), "your eyes\nonly");
        assert_eq!(got.version.unwrap(), "123");
    }

    #[tokio::test]
    async fn test_rules() -> anyhow::Result<()> {
        let base = b"
%include child

[include]
a

[metadata]
title = base
";

        let child = b"
%include grand_child

[include]
b

[metadata]
title = child
";

        let grand_child = b"
[include]
c

[metadata]
title = grand_child
";

        let mut base_prof = Profile::from_bytes(base, "test".to_string()).unwrap();

        let rules = base_prof
            .rules(|path| async move {
                match path.as_ref() {
                    "child" => Ok(child.to_vec()),
                    "grand_child" => Ok(grand_child.to_vec()),
                    _ => Err(anyhow!("not found")),
                }
            })
            .await?;

        assert_eq!(
            rules,
            vec![
                (
                    Pattern::Include("c".to_string()),
                    "test -> child -> grand_child".to_string()
                ),
                (
                    Pattern::Include("b".to_string()),
                    "test -> child".to_string()
                ),
                (Pattern::Include("a".to_string()), "test".to_string())
            ]
        );

        Ok(())
    }

    #[tokio::test]
    async fn test_recursive_imports() {
        let a = b"%include b";
        let b = b"%include a";

        let mut a_prof = Profile::from_bytes(a, "test".to_string()).unwrap();

        let res = a_prof
            .rules(|path| async move {
                match path.as_ref() {
                    "a" => Ok(a.to_vec()),
                    "b" => Ok(b.to_vec()),
                    _ => Err(anyhow!("not found")),
                }
            })
            .await;

        assert_eq!(format!("{}", res.unwrap_err()), "import cycle involving b");
    }

    #[tokio::test]
    async fn test_resolve_imports_caching() {
        let a = b"
%include b
%include b
";

        let mut a_prof = Profile::from_bytes(a, "test".to_string()).unwrap();

        let mut fetch_count = 0;

        // Make sure we cache results from the callback.
        let res = a_prof
            .rules(|_path| {
                fetch_count += 1;
                assert_eq!(fetch_count, 1);
                async { Ok(vec![]) }
            })
            .await;

        assert!(res.is_ok());
    }

    #[test]
    fn test_sparse_pat_to_matcher_rule() {
        assert_eq!(
            sparse_pat_to_matcher_rule(Pattern::Include("path:/foo/bar".to_string())).unwrap(),
            vec!["/foo/bar/**"]
        );

        assert_eq!(
            sparse_pat_to_matcher_rule(Pattern::Include("/foo/*/bar{1,{2,3}}/".to_string()))
                .unwrap(),
            vec!["/foo/*/bar1/**", "/foo/*/bar2/**", "/foo/*/bar3/**"],
        );

        assert_eq!(
            sparse_pat_to_matcher_rule(Pattern::Include("path:/foo/*/bar{1,{2,3}}/".to_string()))
                .unwrap(),
            vec!["/foo/\\*/bar\\{1,\\{2,3\\}\\}/**"],
        );

        assert_eq!(
            sparse_pat_to_matcher_rule(Pattern::Exclude("glob:**".to_string())).unwrap(),
            vec!["!**/**"],
        );

        assert!(sparse_pat_to_matcher_rule(Pattern::Include("re:.*".to_string())).is_err());
    }
}
