// Copyright 2019 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.
use crate::parser::ParseError;
use crate::utils::get_prefix_bounds;
use shlex::split;
use std::collections::{BTreeMap, HashMap, HashSet};

/// Expands all aliases accounting for circular references and prefix matching.
///
/// * `lookup` - A function to expand a command name to another shell-like command.
///
/// * `args` - The original arguments to resolve aliases from.
/// The first argument should be the command name.
///
/// On success, returns a tuple of Vec<String>.  The first is the expanded arguments.  The second
/// is the in-order replacements that were made to get to the expanded arguments.  If the second
/// vector is empty, no replacements were made.  If the second vector has arguments, the 0th index
/// is what the user originally typed.
pub fn expand_aliases<S: ToString>(
    lookup: impl Fn(&str) -> Option<S>,
    args: &[impl ToString],
) -> Result<(Vec<String>, Vec<String>), ParseError> {
    let mut replaced = Vec::new(); // keep track of what is replaced in-order
    let mut visited = HashSet::new();

    let mut args: Vec<String> = args.iter().map(ToString::to_string).collect();
    let mut command_name = args.first().cloned().unwrap_or_default();

    while let Some(alias) = lookup(&command_name) {
        let alias = alias.to_string();
        let bad_alias = || ParseError::IllformedAlias {
            name: command_name.clone(),
            value: alias.to_string(),
        };

        if !visited.insert(command_name.clone()) {
            return Err(ParseError::CircularReference { command_name });
        }
        replaced.push(command_name.clone());

        let alias_args: Vec<String> = split(&alias).ok_or_else(bad_alias)?;
        args = expand_alias_args(&args, alias_args);

        let next_command_name = args.first().cloned().ok_or_else(bad_alias)?;
        if next_command_name == command_name {
            break;
        } else {
            command_name = next_command_name;
        }
    }

    Ok((args, replaced))
}

/// Expand a single alias.
///
/// The first item of both `command_args` and `alias_args` are expected to be
/// command name.
///
/// Usually returns:
///
/// ```plain,ignore
/// alias_args + command_args[1:]
/// ```
///
/// In case there are `$1`, `$2` etc. in `alias_args`, those parts of
/// `alias_args` will be replaced by corrosponding parts of `command_args`, and
/// the result looks like:
///
/// ```plain,ignore
/// alias_name + alias_args (with $x replaced) + command_args[n+1:]
/// ```
///
/// where `n` is the maximum number occured in `$x`.
fn expand_alias_args(command_args: &[String], alias_args: Vec<String>) -> Vec<String> {
    let mut n = 0;
    let mut args: Vec<String> = alias_args
        .into_iter()
        .map(|a| {
            if a.starts_with("$") {
                if let Ok(i) = a[1..].parse::<usize>() {
                    if let Some(existing_arg) = command_args.get(i) {
                        // Found a substitution. Use it.
                        // Also update the maximum number `n`.
                        n = i.max(n);
                        return existing_arg.to_string();
                    }
                }
            }
            a
        })
        .collect();

    if let Some(slice) = command_args.get(n + 1..) {
        args.extend(slice.iter().cloned());
    } else {
        // TODO: This might be an error case.
    }
    args
}

/// Prefix match commands to their full command name.  If a prefix is not unique an Error::AmbiguousCommand
/// will be returned with a vector of possibilities to choose from.
///
/// * `command_map` - The mapping of a command name ( key ) -> some id ( val ).
/// This is important because some command_names are really the same command
/// e.g. 'id' and 'identify'.  If commands point to the same id they are assumed
/// to be equivalent.
///
/// * `arg` - The command prefix to expand.
///
/// If there is an exact match the argument is returned as-is.  
/// If there is no match the argument is returned as-is.
pub fn expand_prefix(
    command_map: &BTreeMap<String, usize>,
    arg: impl ToString,
) -> Result<String, ParseError> {
    let arg = arg.to_string();
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

                return Err(ParseError::AmbiguousCommand {
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
        let cfg: BTreeMap<&'static str, &'static str> = BTreeMap::new();
        let (_expanded, replaced) = expand_aliases(|x| cfg.get(x), &["log"]).unwrap();
        assert!(replaced.len() == 0);
    }

    #[test]
    fn test_one_alias() {
        let mut cfg = BTreeMap::new();
        cfg.insert("log".to_string(), "log -v".to_string());

        let (expanded, replaced) = expand_aliases(|x| cfg.get(x), &["log"]).unwrap();
        assert_eq!(expanded, vec!["log", "-v"]);
        assert_eq!(replaced, vec!["log"]);
    }

    #[test]
    fn test_ambiguous_alias() {
        let mut command_map = BTreeMap::new();
        command_map.insert("foo".to_string(), 0);
        command_map.insert("foobar".to_string(), 1);

        if let Err(err) = expand_prefix(&command_map, "fo") {
            let msg = format!("{}", err);
            assert_eq!(msg, "Command fo is ambiguous");
        } else {
            panic!()
        }
    }

    #[test]
    fn test_ambiguous_command() {
        let mut command_map = BTreeMap::new();
        command_map.insert("foo".to_string(), 0);
        command_map.insert("foobar".to_string(), 1);

        if let Err(err) = expand_prefix(&command_map, "fo") {
            let msg = format!("{}", err);
            assert_eq!(msg, "Command fo is ambiguous");
        } else {
            panic!()
        }
    }

    #[test]
    fn test_ambiguous_command_and_alias() {
        let mut command_map = BTreeMap::new();
        command_map.insert("foobar".to_string(), 0);
        command_map.insert("foo".to_string(), 1);

        if let Err(err) = expand_prefix(&command_map, "fo") {
            let msg = format!("{}", err);
            assert_eq!(msg, "Command fo is ambiguous");
        } else {
            panic!()
        }
    }

    #[test]
    fn test_command_same_handler() {
        let mut command_map = BTreeMap::new();
        command_map.insert("id".to_string(), 0);
        command_map.insert("identify".to_string(), 0);

        let element = expand_prefix(&command_map, "i").unwrap();
        assert!((element == "id") || (element == "identify"));
    }

    #[test]
    fn test_circular_alias() {
        let mut cfg = BTreeMap::new();
        cfg.insert("foo".to_string(), "log".to_string());
        cfg.insert("log".to_string(), "foo".to_string());

        if let Err(err) = expand_aliases(|x| cfg.get(x), &["foo"]) {
            let msg = format!("{}", err);
            assert_eq!(msg, "Alias foo resulted in a circular reference");
        } else {
            panic!()
        }
    }

    #[test]
    fn test_long_alias_chain() {
        let mut cfg = BTreeMap::new();
        cfg.insert("a".to_string(), "b 1".to_string());
        cfg.insert("b".to_string(), "c 2".to_string());
        cfg.insert("c".to_string(), "d 3".to_string());

        let (expanded, _replaced) = expand_aliases(|x| cfg.get(x), &["a"]).unwrap();

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

        let (expanded, replaced) = expand_aliases(|x| cfg.get(x), &["foo"]).unwrap();

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

        let (expanded, replaced) = expand_aliases(|x| cfg.get(x), &["bar"]).unwrap();

        assert_eq!(expanded, vec!["foo", "-r", "foo", "-v", "foo", "foo"]);
        assert_eq!(replaced, vec!["bar", "foo"]);
    }

    #[test]
    fn test_empty_alias() {
        let mut cfg = BTreeMap::new();
        cfg.insert("nodef".to_string(), "".to_string());

        expand_aliases(|x| cfg.get(x), &["nodef"]).unwrap_err();
    }

    #[test]
    fn test_expand_dollar() {
        let mut cfg = BTreeMap::new();
        cfg.insert("a", "b $2 $1");
        cfg.insert("b", "$1 c d $2");
        cfg.insert("y", "Y");

        // Sufficient args
        let (expanded, _replaced) = expand_aliases(|x| cfg.get(x), &["a", "x", "y", "z"]).unwrap();
        // Initial: a x y z
        // Step 1: Rule: a => b $2 $1 => b y x; Result: b y x z q
        // Step 2: Rule: b => $1 c d $2 => y c d x; Result: y c d x z
        // Step 3: Rule: y => Y; Result: Y c d x z
        assert_eq!(expanded, vec!["Y", "c", "d", "x", "z"]);

        // Insufficient args
        let expanded = expand_aliases(|x| cfg.get(x), &["a", "x"]).unwrap().0;
        assert_eq!(expanded, vec!["$2", "c", "d", "x"]);
    }
}
