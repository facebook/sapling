// Copyright 2019 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.
use crate::utils::get_prefix_bounds;
use failure::Fail;
use shlex::split;
use std::collections::{BTreeMap, HashMap, HashSet};

#[derive(Debug, Fail)]
pub enum Error {
    #[fail(display = "Command {} is ambiguous", command_name)]
    AmbiguousCommand {
        command_name: String,
        possibilities: Vec<String>,
    },
    #[fail(display = "Alias {} resulted in a circular reference", command_name)]
    CircularReference { command_name: String },
}

/// Expands all aliases accounting for circular references and prefix matching.
///
/// * `cfg` - The alias mapping of alias name ( key ) -> alias value ( val ).
///
/// * `command_map` - The mapping of a command name ( key ) -> some id ( val ).
/// This is important because some command_names are really the same command
/// e.g. 'id' and 'identify'.  If commands point to the same id they are assumed
/// to be equivalent.
///
/// * `args` - The original arguments to resolve aliases from
///
/// * `strict` - Decides if there should be strict or prefix matching ( True = no prefix matching )
///
/// On success, returns a tuple of Vec<String>.  The first is the expanded arguments.  The second
/// is the in-order replacements that were made to get to the expanded arguments.  If the second
/// vector is empty, no replacements were made.  If the second vector has arguments, the 0th index
/// is what the user originally typed.
pub fn expand_aliases(
    cfg: &BTreeMap<String, String>,
    command_map: &BTreeMap<String, usize>,
    mut arg: String,
    strict: bool,
) -> Result<(Vec<String>, Vec<String>), Error> {
    let mut following_args = Vec::new();
    let mut replaced = Vec::new(); // keep track of what is replaced in-order

    if !strict {
        arg = replace_prefix(command_map, arg)?;
    }

    let mut visited: HashMap<_, usize> = HashMap::new();
    let mut expanded = Vec::new();

    loop {
        if arg.starts_with("-") {
            expanded.push(arg);
            break;
        }

        match visited.get(&arg) {
            Some(amount) => {
                if amount > &1 {
                    return Err(Error::CircularReference { command_name: arg });
                }
            }
            _ => (),
        }

        match cfg.get(&arg) {
            Some(alias) => {
                let parts: Vec<String> = match split(alias) {
                    Some(v) => v,
                    None => {
                        expanded.push(arg);
                        break;
                    }
                };
                *visited.entry(arg.clone()).or_insert(0) += 1;
                replaced.push(arg.clone());

                if parts.len() == 0 {
                    break;
                }
                let len = parts.len() - 1;

                let mut last_part = None;

                for (idx, part) in parts.into_iter().rev().enumerate() {
                    if idx == len {
                        last_part = Some(part);
                    } else {
                        following_args.insert(0, part);
                    }
                }

                match last_part {
                    Some(ref same_arg) if same_arg == &arg => {
                        expanded.push(arg);
                        break;
                    }
                    Some(diff_arg) => arg = diff_arg,
                    None => unreachable!(),
                }
            }
            None => {
                expanded.push(arg);
                break;
            }
        }
    }

    expanded.extend(following_args.into_iter());

    Ok((expanded, replaced))
}

/// Prefix match commands to their full command name.  If a prefix is not unique an Error::AmbiguousCommand
/// will be returned with a vector of possibilities to choose from.
///
/// If there is an exact match the argument is returned as-is.  
/// If there is no match the argument is returned as-is.
fn replace_prefix(command_map: &BTreeMap<String, usize>, arg: String) -> Result<String, Error> {
    let resolved = match command_map.get(&arg) {
        Some(_) => arg,
        None => {
            let command_range = command_map.range(get_prefix_bounds(&arg));

            let command_matches_map: HashMap<&str, &usize> =
                command_range.map(|(c, id)| ((*c).as_ref(), id)).collect();

            let mut seen_ids = HashSet::new();
            let mut command_matches = HashSet::new();
            let mut id_to_command_map = HashMap::new();

            // split commands point to the same handler like id and identify, we only need one
            for (command, id) in &command_matches_map {
                if !seen_ids.contains(&id) {
                    command_matches.insert(*command);
                    seen_ids.insert(id);
                }
                id_to_command_map
                    .entry(id)
                    .or_insert(Vec::new())
                    .push(*command);
            }

            if command_matches.len() > 1 {
                // sort command aliases by length for consistency
                for (_, vec) in &mut id_to_command_map {
                    vec.sort_by_key(|s| s.len());
                }

                // join command aliases with ' or ' for better UX
                // e.g. id or identify
                let possibilities: Vec<String> = id_to_command_map
                    .into_iter()
                    .map(|(_, vec)| vec.join(" or "))
                    .collect();

                return Err(Error::AmbiguousCommand {
                    command_name: arg,
                    possibilities,
                });
            } else if command_matches.len() == 1 {
                let alias = command_matches.into_iter().next().unwrap();
                alias.to_string()
            } else {
                arg
            }
        }
    };

    Ok(resolved)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_no_alias() {
        let cfg = BTreeMap::new();
        let command_map = BTreeMap::new();
        let (_expanded, replaced) =
            expand_aliases(&cfg, &command_map, "log".to_string(), false).unwrap();
        assert!(replaced.len() == 0);
    }

    #[test]
    fn test_one_alias() {
        let mut cfg = BTreeMap::new();
        cfg.insert("log".to_string(), "log -v".to_string());
        let command_map = BTreeMap::new();

        let (expanded, replaced) =
            expand_aliases(&cfg, &command_map, "log".to_string(), false).unwrap();
        assert_eq!(expanded, vec!["log", "-v"]);
        assert_eq!(replaced, vec!["log"]);
    }

    #[test]
    fn test_ambiguous_alias() {
        let cfg = BTreeMap::new();
        let mut command_map = BTreeMap::new();
        command_map.insert("foo".to_string(), 0);
        command_map.insert("foobar".to_string(), 1);

        if let Err(err) = expand_aliases(&cfg, &command_map, "fo".to_string(), false) {
            let msg = format!("{}", err);
            assert_eq!(msg, "Command fo is ambiguous");
        } else {
            panic!()
        }
    }

    #[test]
    fn test_ambiguous_command() {
        let cfg = BTreeMap::new();
        let mut command_map = BTreeMap::new();
        command_map.insert("foo".to_string(), 0);
        command_map.insert("foobar".to_string(), 1);

        if let Err(err) = expand_aliases(&cfg, &command_map, "fo".to_string(), false) {
            let msg = format!("{}", err);
            assert_eq!(msg, "Command fo is ambiguous");
        } else {
            panic!()
        }
    }

    #[test]
    fn test_ambiguous_command_and_alias() {
        let mut cfg = BTreeMap::new();
        let mut command_map = BTreeMap::new();
        cfg.insert("foo".to_string(), "log".to_string());
        command_map.insert("foobar".to_string(), 0);
        command_map.insert("foo".to_string(), 1);

        if let Err(err) = expand_aliases(&cfg, &command_map, "fo".to_string(), false) {
            let msg = format!("{}", err);
            assert_eq!(msg, "Command fo is ambiguous");
        } else {
            panic!()
        }
    }

    #[test]
    fn test_command_same_handler() {
        let cfg = BTreeMap::new();
        let mut command_map = BTreeMap::new();
        command_map.insert("id".to_string(), 0);
        command_map.insert("identify".to_string(), 0);

        let (expanded, _replaced) =
            expand_aliases(&cfg, &command_map, "i".to_string(), false).unwrap();
        let element = expanded.get(0).unwrap();
        assert!((element == "id") || (element == "identify"));
    }

    #[test]
    fn test_circular_alias() {
        let mut cfg = BTreeMap::new();
        let command_map = BTreeMap::new();
        cfg.insert("foo".to_string(), "log".to_string());
        cfg.insert("log".to_string(), "foo".to_string());

        if let Err(err) = expand_aliases(&cfg, &command_map, "foo".to_string(), false) {
            let msg = format!("{}", err);
            assert_eq!(msg, "Alias foo resulted in a circular reference");
        } else {
            panic!()
        }
    }

    #[test]
    fn test_long_alias_chain() {
        let mut cfg = BTreeMap::new();
        let command_map = BTreeMap::new();
        cfg.insert("a".to_string(), "b 1".to_string());
        cfg.insert("b".to_string(), "c 2".to_string());
        cfg.insert("c".to_string(), "d 3".to_string());

        let (expanded, _replaced) =
            expand_aliases(&cfg, &command_map, "a".to_string(), false).unwrap();

        assert_eq!(expanded, vec!["d", "3", "2", "1"]);
    }

    // hg --config "alias.foo=log bar" --config alias.bar=oops --config "alias.log=log -v" foo
    // hg foo -> hg log bar -> hg log -v bar ( if bar changes to oops this is invalid )
    #[test]
    fn test_weird_chain() {
        let mut cfg = BTreeMap::new();
        cfg.insert("foo".to_string(), "log bar".to_string());
        cfg.insert("bar".to_string(), "oops".to_string());
        cfg.insert("log".to_string(), "log -v".to_string());
        let command_map = BTreeMap::new();

        let (expanded, replaced) =
            expand_aliases(&cfg, &command_map, "foo".to_string(), false).unwrap();

        assert_eq!(expanded, vec!["log", "-v", "bar"]);
        assert_eq!(replaced, vec!["foo", "log"]);
    }

    // hg --config "alias.foo=foo -r foo -v foo foo" --config "alias.bar=foo" bar
    // hg bar -> hg foo -r foo -v foo foo ( the multiple foos should not be recursively expanded )
    #[test]
    fn test_multiple_commands_in_args() {
        let mut cfg = BTreeMap::new();
        cfg.insert("foo".to_string(), "foo -r foo -v foo foo".to_string());
        cfg.insert("bar".to_string(), "foo".to_string());
        let command_map = BTreeMap::new();

        let (expanded, replaced) =
            expand_aliases(&cfg, &command_map, "bar".to_string(), false).unwrap();

        assert_eq!(expanded, vec!["foo", "-r", "foo", "-v", "foo", "foo"]);
        assert_eq!(replaced, vec!["bar", "foo"]);
    }

    #[test]
    fn test_empty_alias() {
        let mut cfg = BTreeMap::new();
        cfg.insert("nodef".to_string(), "".to_string());
        let command_map = BTreeMap::new();

        let (expanded, replaced) =
            expand_aliases(&cfg, &command_map, "nodef".to_string(), false).unwrap();
        let empty: Vec<String> = Vec::new();
        assert_eq!(expanded, empty);
        assert_eq!(replaced, vec!["nodef"]);
    }

}
