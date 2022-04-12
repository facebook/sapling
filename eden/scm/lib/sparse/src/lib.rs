/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::io;
use std::io::BufRead;
use std::io::BufReader;

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
}

#[cfg(test)]
mod tests {
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
}
