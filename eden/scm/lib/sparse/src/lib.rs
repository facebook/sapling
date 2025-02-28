/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::collections::VecDeque;
use std::hash::Hash;
use std::hash::Hasher;
use std::io;
use std::io::BufRead;
use std::io::BufReader;

use once_cell::sync::Lazy;
use pathmatcher::DirectoryMatch;
use pathmatcher::Matcher as MatcherTrait;
use pathmatcher::PatternKind;
use pathmatcher::TreeMatcher;
use pathmatcher::UnionMatcher;
use regex::Regex;
use types::RepoPath;

#[cfg(feature = "async")]
mod extra_use {
    pub(crate) use futures::executor;
    pub(crate) use futures::future::BoxFuture;
    pub(crate) use futures::future::FutureExt;
    pub(crate) use futures::Future;
}

#[cfg(not(feature = "async"))]
mod extra_use {
    pub(crate) use rewrite_macros::syncify;
    pub(crate) type BoxFuture<'a, T> = T;
}

use extra_use::*;

#[derive(Default, Debug, Clone)]
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

    case_sensitive: bool,
}

/// Root represents the root sparse profile (usually .hg/sparse).
#[derive(Debug, Hash)]
pub struct Root {
    prof: Profile,
    version_override: Option<String>,
    skip_catch_all: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Pattern {
    Include(String),
    Exclude(String),
}

#[derive(Debug, Clone)]
pub enum ProfileEntry {
    // Pattern plus additional source for this rule (e.g. "hgrc.dynamic").
    Pattern(Pattern, Option<String>),
    ProfileName(String),
    Profile(Profile),
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

    #[error(transparent)]
    GlobsetError(#[from] globset::Error),

    #[error(transparent)]
    Internal(anyhow::Error),
}

#[cfg_attr(not(feature="async"), syncify([B: Future<Output = anyhow::Result<Option<Vec<u8>>>> + Send] => [], [B] => [anyhow::Result<Option<Vec<u8>>>], [Send + Sync] => []))]
impl Root {
    pub fn from_bytes(data: impl AsRef<[u8]>, source: String) -> Result<Self, io::Error> {
        Ok(Self {
            prof: Profile::from_bytes(data, source)?,
            version_override: None,
            skip_catch_all: false,
        })
    }

    /// Load a single top-level-profile as if you had a root profile that simply %include'd it.
    /// This allows you to create an adhoc v2 profile without needing a root config.
    pub fn single_profile(data: impl AsRef<[u8]>, source: String) -> Result<Self, io::Error> {
        Ok(Self {
            prof: Profile {
                source: "dummy root".to_string(),
                entries: vec![ProfileEntry::Profile(Profile::from_bytes(data, source)?)],
                ..Default::default()
            },
            version_override: None,
            skip_catch_all: false,
        })
    }

    pub fn set_version_override(&mut self, version_override: Option<String>) {
        self.version_override = version_override;
    }

    pub fn set_skip_catch_all(&mut self, skip_catch_all: bool) {
        self.skip_catch_all = skip_catch_all;
    }

    pub async fn matcher<B: Future<Output = anyhow::Result<Option<Vec<u8>>>> + Send>(
        &self,
        mut fetch: impl FnMut(String) -> B + Send + Sync,
    ) -> Result<Matcher, Error> {
        let mut matchers: Vec<TreeMatcher> = Vec::new();

        // List of rule origins per-matcher.
        let mut rule_origins: Vec<Vec<String>> = Vec::new();

        let mut rules: VecDeque<(Pattern, String)> = VecDeque::new();

        // Maintain the excludes-come-last ordering.
        let mut push_rule = |(pat, src)| match pat {
            Pattern::Exclude(_) => rules.push_back((pat, src)),
            Pattern::Include(_) => rules.push_front((pat, src)),
        };

        let prepare_rules =
            |rules: VecDeque<(Pattern, String)>| -> Result<(Vec<String>, Vec<String>), Error> {
                let mut matcher_rules = Vec::new();
                let mut origins = Vec::new();

                for (pat, src) in rules {
                    match sparse_pat_to_matcher_rule(&pat) {
                        Err(err) => {
                            tracing::error!(%err, ?pat, %src, "ignoring unsupported sparse pattern");
                        }
                        Ok(rules) => {
                            for expanded_rule in rules {
                                origins.push(format!("{} ({})", expanded_rule, src));
                                matcher_rules.push(expanded_rule);
                            }
                        }
                    }
                }

                Ok((matcher_rules, origins))
            };

        let mut only_v1 = true;
        for entry in self.prof.entries.iter() {
            let mut child = match entry {
                ProfileEntry::Pattern(p, src) => {
                    push_rule((
                        p.clone(),
                        join_source(self.prof.source.clone(), src.as_deref()),
                    ));
                    continue;
                }
                ProfileEntry::ProfileName(child_path) => match fetch(child_path.clone()).await? {
                    Some(data) => Profile::from_bytes(data, child_path.clone())?,
                    None => continue,
                },
                ProfileEntry::Profile(prof) => prof.clone(),
            };

            if let Some(version_override) = &self.version_override {
                child.version = Some(version_override.clone());
            }

            let child_rules: VecDeque<(Pattern, String)> = child
                .rules(&mut fetch)
                .await?
                .into_iter()
                .map(|(p, s)| (p, format!("{} -> {}", self.prof.source, s)))
                .collect();

            if child.is_v2() {
                only_v1 = false;

                let (matcher_rules, origins) = prepare_rules(child_rules)?;
                matchers.push(
                    build_tree_matcher_from_rules(matcher_rules, self.prof.case_sensitive).await?,
                );
                rule_origins.push(origins);
            } else {
                for rule in child_rules {
                    push_rule(rule);
                }
            }
        }

        // If all user specified rules are exclude rules, add an
        // implicit "**" to provide the default include of everything.
        if only_v1
            && (rules.is_empty() || matches!(&rules[0].0, Pattern::Exclude(_)))
            && !self.skip_catch_all
        {
            rules.push_front((Pattern::Include("**".to_string()), "<builtin>".to_string()))
        }

        // This is for files such as .hgignore and .hgsparse-base, unrelated to the .hg directory.
        rules.push_front((
            Pattern::Include("glob:.hg*".to_string()),
            "<builtin>".to_string(),
        ));

        let (matcher_rules, origins) = prepare_rules(rules)?;
        matchers
            .push(build_tree_matcher_from_rules(matcher_rules, self.prof.case_sensitive).await?);
        rule_origins.push(origins);

        Ok(Matcher::new(matchers, rule_origins))
    }

    // Returns true if the profile excludes the given path.
    pub fn is_path_excluded(self: &Root, path: &str) -> bool {
        // TODO(cuev): Add a warning when sparse profiles contain a %include.
        // Filters don't support that.
        let matcher =
            executor::block_on(self.matcher(|_| async move { Ok(Some(vec![])) })).unwrap();
        let repo_path = RepoPath::from_str(path).unwrap();
        !matcher.matches(repo_path).unwrap_or(true)
    }
}

#[cfg(not(feature = "async"))]
fn build_tree_matcher_from_rules(
    matcher_rules: Vec<String>,
    case_sensitive: bool,
) -> Result<TreeMatcher, Error> {
    Ok(TreeMatcher::from_rules(
        matcher_rules.iter(),
        case_sensitive,
    )?)
}

#[cfg(feature = "async")]
async fn build_tree_matcher_from_rules(
    matcher_rules: Vec<String>,
    case_sensitive: bool,
) -> Result<TreeMatcher, Error> {
    let matcher = tokio::task::spawn_blocking(move || {
        TreeMatcher::from_rules(matcher_rules.iter(), case_sensitive)
    })
    .await
    .map_err(|e| Error::Internal(e.into()))?;
    Ok(matcher?)
}

#[cfg_attr(not(feature="async"), syncify([B: Future<Output = anyhow::Result<Option<Vec<u8>>>> + Send] => [], [B] => [anyhow::Result<Option<Vec<u8>>>], [Send + Sync] => []))]
impl Profile {
    fn from_bytes(data: impl AsRef<[u8]>, source: String) -> Result<Self, io::Error> {
        let mut prof = Profile {
            // Case insensitive matcher support breaks w/ too many rules, so
            // leave it disabled for now. This may need to be fixed if sparse
            // profiles are supposed to support case insensitivity.
            case_sensitive: true,
            ..Default::default()
        };
        let mut current_metadata_val: Option<&mut String> = None;
        let mut section_type = SectionType::Include;
        let mut dynamic_source: Option<String> = None;
        let mut dummy_metadata_value: Option<String> = None;

        for (mut line_num, line) in BufReader::new(data.as_ref()).lines().enumerate() {
            line_num += 1;

            let line = line?;
            let trimmed = line.trim();

            // Ignore comments and empty lines.
            let mut chars = trimmed.chars();
            match chars.next() {
                None => continue,
                Some('#' | ';') => {
                    let comment = chars.as_str().trim();
                    if let Some((l, r)) = comment.split_once(['=', ':']) {
                        match (l.trim(), r.trim()) {
                            // Allow a magic comment to specify additional
                            // source information for particular rules. This way
                            // it is backwards compatible with the python code
                            // if a config like this ever gets written out.
                            ("source", "") => dynamic_source = None,
                            ("source", src) => dynamic_source = Some(src.to_string()),
                            _ => {}
                        }
                    }
                    continue;
                }
                _ => {}
            }

            if let Some(p) = trimmed.strip_prefix("%include ") {
                let p = p.trim();
                if p.ends_with('/') {
                    tracing::warn!(%line, %source, line_num, "ignoring sparse %include ending with /");
                    continue;
                }

                prof.entries
                    .push(ProfileEntry::ProfileName(p.trim().to_string()));
            } else if let Some(section_start) = SectionType::from_str(trimmed) {
                section_type = section_start;
                current_metadata_val = None;
            } else if section_type == SectionType::Metadata {
                if line.starts_with([' ', '\t']) {
                    // Continuation of multiline value.
                    if let Some(ref mut val) = current_metadata_val {
                        val.push('\n');
                        val.push_str(trimmed);
                    } else {
                        tracing::warn!(%line, %source, line_num, "orphan metadata line");
                    }
                } else {
                    current_metadata_val = None;
                    if let Some((key, val)) = trimmed.split_once(['=', ':']) {
                        let prof_val = match key.trim() {
                            "description" => &mut prof.description,
                            "title" => &mut prof.title,
                            "hidden" => &mut prof.hidden,
                            "version" => &mut prof.version,
                            _ => {
                                tracing::info!(%line, %source, line_num, "ignoring uninteresting metadata key");
                                // Use a dummy value to maintain parser state (i.e. avoid
                                // "orphan metadata line" warning).
                                dummy_metadata_value.take();
                                &mut dummy_metadata_value
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
                    prof.entries.push(ProfileEntry::Pattern(
                        Pattern::Include(trimmed.to_string()),
                        dynamic_source.clone(),
                    ));
                } else {
                    prof.entries.push(ProfileEntry::Pattern(
                        Pattern::Exclude(trimmed.to_string()),
                        dynamic_source.clone(),
                    ));
                }
            }
        }

        prof.source = source;

        Ok(prof)
    }

    fn is_v2(&self) -> bool {
        if let Some(version) = &self.version {
            version == "2"
        } else {
            false
        }
    }

    // Recursively flatten this profile into a DFS ordered list of rules.
    // %import statements are resolved by fetching the imported profile's
    // contents using the fetch callback. Returns a vec of each Pattern paired
    // with a String describing its provenance.
    async fn rules<B: Future<Output = anyhow::Result<Option<Vec<u8>>>> + Send>(
        &self,
        mut fetch: impl FnMut(String) -> B + Send + Sync,
    ) -> Result<Vec<(Pattern, String)>, Error> {
        fn rules_inner<'a, B: Future<Output = anyhow::Result<Option<Vec<u8>>>> + Send>(
            prof: &'a Profile,
            fetch: &'a mut (dyn FnMut(String) -> B + Send + Sync),
            rules: &'a mut Vec<(Pattern, String)>,
            source: Option<&'a str>,
            // path => (contents, in_progress)
            seen: &'a mut HashMap<String, (Vec<u8>, bool)>,
        ) -> BoxFuture<'a, Result<(), Error>> {
            async move {
                let source = match source {
                    Some(history) => format!("{} -> {}", history, prof.source),
                    None => prof.source.clone(),
                };

                for entry in prof.entries.iter() {
                    match entry {
                        ProfileEntry::Pattern(p, psrc) => {
                            rules.push((p.clone(), join_source(source.clone(), psrc.as_deref())))
                        }
                        ProfileEntry::ProfileName(child_path) => {
                            let entry = seen.entry(child_path.clone());
                            let data = match entry {
                                Entry::Occupied(e) => match e.into_mut() {
                                    (_, true) => {
                                        return Err(Error::ImportCycle(child_path.clone()));
                                    }
                                    (data, false) => data,
                                },
                                Entry::Vacant(e) => {
                                    if let Some(data) = fetch(child_path.clone()).await? {
                                        &e.insert((data, true)).0
                                    } else {
                                        continue;
                                    }
                                }
                            };

                            let child = Profile::from_bytes(data, child_path.clone())?;
                            rules_inner(&child, fetch, rules, Some(&source), seen).await?;

                            if let Some((_, in_progress)) = seen.get_mut(child_path) {
                                *in_progress = false;
                            }
                        }
                        ProfileEntry::Profile(child) => {
                            rules_inner(child, fetch, rules, Some(&source), seen).await?;
                        }
                    }
                }

                Ok(())
            }
            .boxed()
        }

        let mut rules = Vec::new();
        rules_inner(self, &mut fetch, &mut rules, None, &mut HashMap::new()).await?;
        Ok(rules)
    }
}

impl Hash for Profile {
    fn hash<H>(&self, state: &mut H)
    where
        H: Hasher,
    {
        for entry in self.entries.iter() {
            if let ProfileEntry::Pattern(pat, _) = entry {
                match pat {
                    Pattern::Include(_) => "include",
                    Pattern::Exclude(_) => "exclude",
                }
                .hash(state);
                pat.as_str().hash(state);
            }
        }
    }
}

fn join_source(main_source: String, opt_source: Option<&str>) -> String {
    match opt_source {
        None => main_source,
        Some(opt) => format!("{} ({})", main_source, opt),
    }
}

pub struct Matcher {
    matchers: Vec<TreeMatcher>,
    // List of rule origins per-matcher.
    rule_origins: Vec<Vec<String>>,
}

impl Matcher {
    pub fn matches(&self, path: &RepoPath) -> anyhow::Result<bool> {
        let result = UnionMatcher::matches_file(self.matchers.iter(), path);
        tracing::trace!(%path, ?result, "matches");
        result
    }

    pub fn explain(&self, path: &RepoPath) -> anyhow::Result<(bool, String)> {
        for (i, m) in self.matchers.iter().enumerate() {
            if let Some(idx) = m.matching_rule_indexes(path.as_str()).last() {
                let rule_origin = self
                    .rule_origins
                    .get(i)
                    .and_then(|o| o.get(*idx))
                    .map_or("(unknown)".to_string(), |o| o.clone());
                return Ok((m.matches(path.as_str()), rule_origin));
            }
        }

        Ok((false, "no rules matched".to_string()))
    }

    pub fn into_matchers(self) -> Vec<(TreeMatcher, Vec<String>)> {
        self.matchers.into_iter().zip(self.rule_origins).collect()
    }
}

impl MatcherTrait for Matcher {
    fn matches_directory(&self, path: &RepoPath) -> anyhow::Result<DirectoryMatch> {
        let result = UnionMatcher::matches_directory(self.matchers.iter(), path);
        tracing::trace!(%path, ?result, "matches_directory");
        result
    }

    fn matches_file(&self, path: &RepoPath) -> anyhow::Result<bool> {
        self.matches(path)
    }
}

impl Matcher {
    pub fn new(matchers: Vec<TreeMatcher>, rule_origins: Vec<Vec<String>>) -> Self {
        Self {
            matchers,
            rule_origins,
        }
    }
}

// Convert a sparse profile pattern into what the tree matcher
// expects. We only support "glob" and "path" pattern types.
fn sparse_pat_to_matcher_rule(pat: &Pattern) -> Result<Vec<String>, Error> {
    let (pat_type, pat_text) = pathmatcher::split_pattern(pat.as_str(), PatternKind::Glob);
    match pat_type {
        PatternKind::Glob | PatternKind::Path => {} // empty
        PatternKind::RE => match convert_regex_to_glob(pat_text) {
            Some(globs) => {
                return Ok(globs);
            }
            None => return Err(Error::UnsupportedPattern(pat_type.name().to_string())),
        },
        _ => {
            return Err(Error::UnsupportedPattern(pat_type.name().to_string()));
        }
    };

    let pats = match pat_type {
        PatternKind::Glob => pathmatcher::expand_curly_brackets(pat_text)
            .iter()
            .map(|s| pathmatcher::normalize_glob(s))
            .collect(),
        PatternKind::Path => vec![pathmatcher::normalize_glob(
            pathmatcher::plain_to_glob(pat_text).as_str(),
        )],
        _ => unreachable!(),
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

fn make_recursive(p: impl Into<String>) -> String {
    let p = p.into();
    if p.is_empty() || p.ends_with('/') {
        p + "**"
    } else {
        p + "/**"
    }
}

// Match patterns like "foo/(?!bar/)" which mean "include foo/ except foo/bar/".
static EXCLUDE_DIR_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^\^?([\w._/]+)/\(\?!([\w._/]+)\)$").unwrap());

// Match patterns like "foo/(?:.*/)?bar(?:/|$)".
static ANY_DIR_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^\^?([\w._/]+)/\(\?:\.\*/\)\?([\w._]+)(\(\?:/\|\$\))?$").unwrap());

// Match patterns like "foo/\..*/ to match dotfiles.
static DOT_FILES_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"^\^?([\w._/]+)/\\.\.\*$").unwrap());

// Attempt to convert given regex pattern to glob(s). Only certain
// cases are handled to give best effort support for checking out
// older commits that still use regex patterns.
fn convert_regex_to_glob(pat: &str) -> Option<Vec<String>> {
    if let Some(caps) = EXCLUDE_DIR_RE.captures(pat) {
        let prefix = caps.get(1).unwrap().as_str();
        let excluded = caps.get(2).unwrap().as_str();
        return Some(vec![
            make_recursive(prefix),
            make_recursive(format!("!{}/{}", prefix, excluded)),
        ]);
    }

    if let Some(caps) = ANY_DIR_RE.captures(pat) {
        let prefix = caps.get(1).unwrap().as_str();
        let name = caps.get(2).unwrap().as_str();

        // Turn trailing (?:/|$) into trailing "/**".
        let end = match caps.get(3) {
            Some(_) => "/**",
            None => "",
        };
        return Some(vec![format!("{}/**/{}{}", prefix, name, end)]);
    }

    if let Some(caps) = DOT_FILES_RE.captures(pat) {
        let prefix = caps.get(1).unwrap().as_str();
        return Some(vec![format!("{}/.*/**", prefix)]);
    }

    None
}

#[cfg(test)]
#[cfg_attr(not(feature = "async"), syncify)]
mod tests {
    use anyhow::anyhow;

    use super::*;

    // Returns a profile's (includes, excludes, profiles).
    fn split_prof(prof: &Profile) -> (Vec<&str>, Vec<&str>, Vec<&str>) {
        let (mut inc, mut exc, mut profs) = (vec![], vec![], vec![]);
        for entry in &prof.entries {
            match entry {
                ProfileEntry::Pattern(Pattern::Include(p), _) => inc.push(p.as_ref()),
                ProfileEntry::Pattern(Pattern::Exclude(p), _) => exc.push(p.as_ref()),
                ProfileEntry::ProfileName(p) => profs.push(p.as_ref()),
                ProfileEntry::Profile(p) => profs.push(p.source.as_ref()),
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

        let base_prof = Profile::from_bytes(base, "test".to_string()).unwrap();

        let rules = base_prof
            .rules(|path| async move {
                match path.as_ref() {
                    "child" => Ok(Some(child.to_vec())),
                    "grand_child" => Ok(Some(grand_child.to_vec())),
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

        let a_prof = Profile::from_bytes(a, "test".to_string()).unwrap();

        let res = a_prof
            .rules(|path| async move {
                match path.as_ref() {
                    "a" => Ok(Some(a.to_vec())),
                    "b" => Ok(Some(b.to_vec())),
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

        let a_prof = Profile::from_bytes(a, "test".to_string()).unwrap();

        let mut fetch_count = 0;

        // Make sure we cache results from the callback.
        let res = a_prof
            .rules(|_path| {
                fetch_count += 1;
                assert_eq!(fetch_count, 1);
                async { Ok(Some(vec![])) }
            })
            .await;

        assert!(res.is_ok());
    }

    #[test]
    fn test_sparse_pat_to_matcher_rule() {
        assert_eq!(
            sparse_pat_to_matcher_rule(&Pattern::Include("a_valid:path/bar".to_string())).unwrap(),
            vec!["a_valid:path/bar/**"]
        );

        assert_eq!(
            sparse_pat_to_matcher_rule(&Pattern::Include("path:/foo/bar".to_string())).unwrap(),
            vec!["/foo/bar/**"]
        );

        assert_eq!(
            sparse_pat_to_matcher_rule(&Pattern::Include("path:/foo//bar".to_string())).unwrap(),
            vec!["/foo/bar/**"]
        );

        assert_eq!(
            sparse_pat_to_matcher_rule(&Pattern::Include("/foo/*/bar{1,{2,3}}/".to_string()))
                .unwrap(),
            vec!["/foo/*/bar1/**", "/foo/*/bar2/**", "/foo/*/bar3/**"],
        );

        assert_eq!(
            sparse_pat_to_matcher_rule(&Pattern::Include("path:/foo/*/bar{1,{2,3}}/".to_string()))
                .unwrap(),
            vec!["/foo/\\*/bar\\{1,\\{2,3\\}\\}/**"],
        );

        assert_eq!(
            sparse_pat_to_matcher_rule(&Pattern::Exclude("glob:**".to_string())).unwrap(),
            vec!["!**/**"],
        );

        assert!(sparse_pat_to_matcher_rule(&Pattern::Include("re:.*".to_string())).is_err());

        assert_eq!(
            sparse_pat_to_matcher_rule(&Pattern::Include(r"re:foo/\..*".to_string())).unwrap(),
            vec![r"foo/.*/**"],
        );

        assert_eq!(
            sparse_pat_to_matcher_rule(&Pattern::Include(r"re:foo/(?!bar/)".to_string())).unwrap(),
            vec![r"foo/**", "!foo/bar/**"],
        );

        assert_eq!(
            sparse_pat_to_matcher_rule(&Pattern::Include(
                r"re:foo/(?:.*/)?baz.txt(?:/|$)".to_string()
            ))
            .unwrap(),
            vec!["foo/**/baz.txt/**"],
        );

        // Don't unescape asterisks accidentally.
        assert!(sparse_pat_to_matcher_rule(&Pattern::Include(r"re:\*".to_string())).is_err());

        // Giver up on regex exclude patterns.
        assert!(sparse_pat_to_matcher_rule(&Pattern::Exclude(r"re:foo".to_string())).is_err());
    }

    #[tokio::test]
    async fn test_matcher_implicit_include() -> anyhow::Result<()> {
        let config = b"
[exclude]
path:exc
";

        let prof = Root::from_bytes(config, "test".to_string()).unwrap();

        let matcher = prof.matcher(|_| async { Ok(Some(vec![])) }).await?;

        // Show we got an implicit rule that includes everything.
        assert!(matcher.matches("a/b".try_into()?)?);

        // Sanity that exclude works.
        assert!(!matcher.matches("exc/foo".try_into()?)?);

        Ok(())
    }

    #[tokio::test]
    async fn test_matcher_v1() -> anyhow::Result<()> {
        let base = b"
%include child

[exclude]
path:a/exc

[include]
path:a
";

        let child = b"
[exclude]
path:b/exc

[include]
path:b
";

        let prof = Root::from_bytes(base, "test".to_string())?;
        let matcher = prof.matcher(|_| async { Ok(Some(child.to_vec())) }).await?;

        // Exclude rule "wins" for v1 despite order in confing.
        assert!(!matcher.matches("a/exc".try_into()?)?);
        assert!(!matcher.matches("b/exc".try_into()?)?);
        assert!(matcher.matches("a/inc".try_into()?)?);
        assert!(matcher.matches("b/inc".try_into()?)?);

        Ok(())
    }

    #[tokio::test]
    async fn test_matcher_v2() -> anyhow::Result<()> {
        let base = b"
%include child_1
%include child_2

[exclude]
path:a/exc
path:c

[include]
path:a
";

        let child_1 = b"
[include]
path:c

[metadata]
version = 2
";

        let child_2 = b"
[exclude]
path:b/exc
path:c

[include]
path:b

[metadata]
version = 2
";

        let prof = Root::from_bytes(base, "test".to_string())?;
        let matcher = prof
            .matcher(|path| async move {
                match path.as_ref() {
                    "child_1" => Ok(Some(child_1.to_vec())),
                    "child_2" => Ok(Some(child_2.to_vec())),
                    _ => unreachable!(),
                }
            })
            .await?;

        // Rules directly in root profile still get excludes-go-last ordering.
        assert!(!matcher.matches("a/exc".try_into()?)?);
        assert!(matcher.matches("a/inc".try_into()?)?);

        // Order for v2 child profile is maintained - include rule wins.
        assert!(matcher.matches("b/exc".try_into()?)?);
        assert!(matcher.matches("b/inc".try_into()?)?);

        // "c" is included due to unioning of v2 profiles.
        assert!(matcher.matches("c".try_into()?)?);

        Ok(())
    }

    #[tokio::test]
    async fn test_matcher_missing_include() -> anyhow::Result<()> {
        let config = b"
%include banana
foo
";

        let prof = Root::from_bytes(config, "test".to_string()).unwrap();

        let matcher = prof.matcher(|_| async { Ok(None) }).await?;

        // We ignore missing includes so that things don't completely
        // break if someone accidentally deletes an in-use sparse
        // profile.
        assert!(matcher.matches("foo".try_into()?)?);

        Ok(())
    }

    #[tokio::test]
    async fn test_matcher_unsupported_patterns() -> anyhow::Result<()> {
        let config = b"
re:.*
listfile0:/tmp/oops
foo
";

        let prof = Root::from_bytes(config, "test".to_string()).unwrap();

        // Can still get a matcher, skipping unsupported patterns.
        let matcher = prof.matcher(|_| async { Ok(None) }).await?;

        assert!(matcher.matches("foo".try_into()?)?);
        assert!(!matcher.matches("bar".try_into()?)?);

        Ok(())
    }

    #[tokio::test]
    async fn test_regex_patterns() -> anyhow::Result<()> {
        let config = b"
[metadata]
version = 2

[include]
re:foo/\\..*
re:^bar/(?!bad/)
re:^bar/bad/(?:.*/)?IMPORTANT.ext(?:/|$)
";

        let prof = Root::from_bytes(b"%include foo", "test".to_string()).unwrap();
        let matcher = prof
            .matcher(|_| async { Ok(Some(config.to_vec())) })
            .await?;

        assert!(matcher.matches("foo/.blah".try_into()?)?);
        assert!(!matcher.matches("foo/not-dot".try_into()?)?);

        assert!(matcher.matches("bar/ok".try_into()?)?);
        assert!(!matcher.matches("bar/bad/nono".try_into()?)?);
        assert!(matcher.matches("bar/bad/well/jk/IMPORTANT.ext".try_into()?)?);

        Ok(())
    }

    #[tokio::test]
    async fn test_explain_empty() {
        let prof = Root::from_bytes(b"", "test".to_string()).unwrap();
        let matcher = prof
            .matcher(|_| async move { Ok(Some(vec![])) })
            .await
            .unwrap();

        assert_eq!(
            matcher.explain("a/b".try_into().unwrap()).unwrap(),
            (true, "**/** (<builtin>)".to_string())
        );
    }

    #[tokio::test]
    async fn test_explain_no_match() {
        let prof = Root::from_bytes(b"a", "test".to_string()).unwrap();
        let matcher = prof
            .matcher(|_| async move { Ok(Some(vec![])) })
            .await
            .unwrap();

        assert_eq!(
            matcher.explain("b".try_into().unwrap()).unwrap(),
            (false, "no rules matched".to_string())
        );
    }

    #[tokio::test]
    async fn test_explain_chain() {
        let base = b"%include child_1";
        let child_1 = b"%include child_2";
        let child_2 = b"
[include]
glob:{a,b,c}

[exclude]
path:d
";

        let prof = Root::from_bytes(base, "base".to_string()).unwrap();
        let matcher = prof
            .matcher(|path| async move {
                match path.as_ref() {
                    "child_1" => Ok(Some(child_1.to_vec())),
                    "child_2" => Ok(Some(child_2.to_vec())),
                    _ => unreachable!(),
                }
            })
            .await
            .unwrap();

        assert_eq!(
            matcher.explain("b".try_into().unwrap()).unwrap(),
            (true, "b/** (base -> child_1 -> child_2)".to_string())
        );

        assert_eq!(
            matcher.explain("d".try_into().unwrap()).unwrap(),
            (false, "!d/** (base -> child_1 -> child_2)".to_string())
        );
    }

    #[tokio::test]
    async fn test_dynamic_rule_source() {
        let config = b"
one

# source = banana
two
three

# source =
four
";

        let prof = Root::from_bytes(config, "base".to_string()).unwrap();

        let matcher = prof.matcher(|_| async { Ok(Some(vec![])) }).await.unwrap();

        assert_eq!(
            matcher.explain("one".try_into().unwrap()).unwrap(),
            (true, "one/** (base)".to_string())
        );

        assert_eq!(
            matcher.explain("two".try_into().unwrap()).unwrap(),
            (true, "two/** (base (banana))".to_string())
        );

        assert_eq!(
            matcher.explain("three".try_into().unwrap()).unwrap(),
            (true, "three/** (base (banana))".to_string())
        );

        assert_eq!(
            matcher.explain("four".try_into().unwrap()).unwrap(),
            (true, "four/** (base)".to_string())
        );
    }

    #[tokio::test]
    async fn test_skip_catch_all() {
        let base = b"[exclude]\nfoo";
        let mut prof = Root::from_bytes(base, "base".to_string()).unwrap();

        let matcher = prof.matcher(|_| async { unreachable!() }).await.unwrap();
        assert!(matcher.matches("bar".try_into().unwrap()).unwrap());

        prof.set_skip_catch_all(true);
        let matcher = prof.matcher(|_| async { unreachable!() }).await.unwrap();
        assert!(!matcher.matches("bar".try_into().unwrap()).unwrap());

        // Skip catch-all for empty profile as well.
        let base = b"";
        let mut prof = Root::from_bytes(base, "base".to_string()).unwrap();
        prof.set_skip_catch_all(true);
        let matcher = prof.matcher(|_| async { unreachable!() }).await.unwrap();
        assert!(!matcher.matches("bar".try_into().unwrap()).unwrap());
    }

    #[tokio::test]
    async fn test_version_override() {
        let base = b"
%include child_1
%include child_2
";
        let child_1 = b"
[metadata]
version = 2

[include]
path:foo
";
        let child_2 = b"
[metadata]
version = 2

[exclude]
path:foo
";

        let mut prof = Root::from_bytes(base, "base".to_string()).unwrap();

        let matcher = prof
            .matcher(|path| async move {
                match path.as_ref() {
                    "child_1" => Ok(Some(child_1.to_vec())),
                    "child_2" => Ok(Some(child_2.to_vec())),
                    _ => unreachable!(),
                }
            })
            .await
            .unwrap();
        assert!(matcher.matches("foo".try_into().unwrap()).unwrap());

        prof.set_version_override(Some("1".to_string()));

        let matcher = prof
            .matcher(|path| async move {
                match path.as_ref() {
                    "child_1" => Ok(Some(child_1.to_vec())),
                    "child_2" => Ok(Some(child_2.to_vec())),
                    _ => unreachable!(),
                }
            })
            .await
            .unwrap();
        assert!(!matcher.matches("foo".try_into().unwrap()).unwrap());
    }

    #[tokio::test]
    async fn test_single_profile() {
        let single = b"
[metadata]
version = 2

[exclude]
foo/bar

[include]
foo
";

        // Sanity check that normal loading of this profile does not get v2 semantics.
        let not_single = Root::from_bytes(single, "base".to_string()).unwrap();
        let matcher = not_single
            .matcher(|_| async { unreachable!() })
            .await
            .unwrap();
        assert!(!matcher.matches("foo/bar".try_into().unwrap()).unwrap());

        // Using single_profile gets v2 semantics.
        let single = Root::single_profile(single, "base".to_string()).unwrap();
        let matcher = single.matcher(|_| async { unreachable!() }).await.unwrap();
        assert!(matcher.matches("foo/bar".try_into().unwrap()).unwrap());
    }
}
