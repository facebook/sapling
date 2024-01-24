/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Error;
use mercurial_types::NonRootMPath;

pub type Chunker<T> = Box<dyn Fn(Vec<T>) -> Vec<Vec<T>>>;

/// Build a "chunking hint" out of a string hint with `\n`-separated
/// lists of ','-separated lists of path prefixes
/// The intended use-case is to run something like:
/// `parse_chunking_hint(read_to_string(path)?)?`
/// Chunking hint is a list of lists of `NonRootMPath`, which
/// allows a `Chunker` to group `NonRootMPath`s which start with prefixes
/// from a given list into a single chunk.
pub fn parse_chunking_hint(hint: String) -> Result<Vec<Vec<NonRootMPath>>, Error> {
    hint.split('\n')
        .filter_map(|line| {
            let line = line.trim_matches(|c| c == ' ' || c == '\n');
            if !line.is_empty() {
                let v: Result<Vec<NonRootMPath>, Error> = line
                    .split(',')
                    .filter_map(|prefix| {
                        let trimmed = prefix.trim_matches(|c| c == ' ' || c == '\n');
                        if !trimmed.is_empty() {
                            Some(NonRootMPath::new(trimmed))
                        } else {
                            None
                        }
                    })
                    .collect();

                Some(v)
            } else {
                None
            }
        })
        .collect()
}

/// Build a `Chunker<NonRootMPath>` from a "chunking hint"
/// See `parse_chunking_hint` docstrign for details
pub fn path_chunker_from_hint(
    prefix_lists: Vec<Vec<NonRootMPath>>,
) -> Result<Chunker<NonRootMPath>, Error> {
    Ok(Box::new(move |mpaths| {
        // In case some paths don't match any prefix, let's just put
        // them all in a separate commit
        let mut res: Vec<Vec<NonRootMPath>> = vec![vec![]; prefix_lists.len() + 1];
        let last_index = prefix_lists.len();

        for mpath in mpaths {
            // we need to find if `mpath` fits into any of the `prefix_lists`
            match prefix_lists.iter().position(|prefix_list| {
                prefix_list
                    .iter()
                    .any(|prefix| NonRootMPath::is_prefix_of_opt(Some(prefix), &mpath))
            }) {
                Some(chunk_index) => {
                    // we need to put this `mpath` into `chunk_index` position
                    res[chunk_index].push(mpath);
                }
                None => {
                    // Does not belong to any prefix list, let's put it into the
                    // last chunk
                    res[last_index].push(mpath);
                }
            }
        }

        // Let's clean up potential empty chunks
        let res: Vec<Vec<NonRootMPath>> = res.into_iter().filter(|v| !v.is_empty()).collect();

        res
    }))
}

pub fn even_chunker_with_max_size<T: Clone>(max_chunk_size: usize) -> Result<Chunker<T>, Error> {
    Ok(Box::new(move |items| {
        let res: Vec<Vec<T>> = items
            .chunks(max_chunk_size)
            .map(|chunk| chunk.to_vec())
            .collect();
        res
    }))
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_parse_chunking_hint() {
        let hint = r#"/a/b, /a/c

        /a/d,

        "#;

        let parsed = parse_chunking_hint(hint.to_string()).unwrap();
        assert_eq!(
            parsed,
            vec![
                vec![
                    NonRootMPath::new("/a/b").unwrap(),
                    NonRootMPath::new("/a/c").unwrap()
                ],
                vec![NonRootMPath::new("/a/d").unwrap()]
            ]
        );

        let hint = "/a/b";
        let parsed = parse_chunking_hint(hint.to_string()).unwrap();
        assert_eq!(parsed, vec![vec![NonRootMPath::new("/a/b").unwrap()]]);
    }

    #[test]
    fn test_path_chunked_form_hint() {
        let hint = parse_chunking_hint(
            r#"
            /a/b, /a/c
            /a/d, /b
        "#
            .to_string(),
        )
        .unwrap();

        let chunker = path_chunker_from_hint(hint).unwrap();
        let mpaths: Vec<NonRootMPath> = vec!["/d/e/f", "/a", "/a/b/c", "/a/c", "/b/w/z", "/a/d"]
            .into_iter()
            .map(|p| NonRootMPath::new(p).unwrap())
            .collect();
        let chunked = chunker(mpaths);

        let expeected_chunk_0: Vec<NonRootMPath> = vec!["/a/b/c", "/a/c"]
            .into_iter()
            .map(|p| NonRootMPath::new(p).unwrap())
            .collect();
        let expeected_chunk_1: Vec<NonRootMPath> = vec!["/b/w/z", "/a/d"]
            .into_iter()
            .map(|p| NonRootMPath::new(p).unwrap())
            .collect();
        let expeected_chunk_2: Vec<NonRootMPath> = vec!["/d/e/f", "/a"]
            .into_iter()
            .map(|p| NonRootMPath::new(p).unwrap())
            .collect();
        assert_eq!(
            chunked,
            vec![expeected_chunk_0, expeected_chunk_1, expeected_chunk_2]
        )
    }

    #[test]
    fn test_path_chunked_form_hint_with_empty() {
        let hint = parse_chunking_hint(
            r#"
            /a/b, /a/c
            /ababagalamaga
            /a/d, /b
            /a, /d/e/f
        "#
            .to_string(),
        )
        .unwrap();

        let chunker = path_chunker_from_hint(hint).unwrap();
        let mpaths: Vec<NonRootMPath> = vec!["/d/e/f", "/a", "/a/b/c", "/a/c", "/b/w/z", "/a/d"]
            .into_iter()
            .map(|p| NonRootMPath::new(p).unwrap())
            .collect();
        let chunked = chunker(mpaths);

        let expeected_chunk_0: Vec<NonRootMPath> = vec!["/a/b/c", "/a/c"]
            .into_iter()
            .map(|p| NonRootMPath::new(p).unwrap())
            .collect();
        let expeected_chunk_1: Vec<NonRootMPath> = vec!["/b/w/z", "/a/d"]
            .into_iter()
            .map(|p| NonRootMPath::new(p).unwrap())
            .collect();
        let expeected_chunk_2: Vec<NonRootMPath> = vec!["/d/e/f", "/a"]
            .into_iter()
            .map(|p| NonRootMPath::new(p).unwrap())
            .collect();
        assert_eq!(
            chunked,
            vec![expeected_chunk_0, expeected_chunk_1, expeected_chunk_2]
        )
    }
}
