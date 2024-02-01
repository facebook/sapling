/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use configmodel::Config;
use configmodel::ConfigExt;
use minibytes::Text;

#[derive(Debug)]
struct Hook {
    name: Text,
    priority: i64,
    background: bool,
    typ: HookType,
}

#[derive(Debug, PartialEq)]
enum HookType {
    Shell(Text),
    Python(Text),
}

const PY_PREFIX: &str = "python:";
const BG_PREFIX: &str = "background:";

fn hooks_from_config(cfg: &dyn Config, hook_name_prefix: &str) -> Vec<Hook> {
    let mut hooks = Vec::new();

    for name in cfg.keys("hooks") {
        if name != hook_name_prefix && !name.starts_with(&format!("{hook_name_prefix}.")) {
            continue;
        }

        let Some(value) = cfg.get_nonempty("hooks", &name) else {
            continue;
        };

        let priority: i64 = cfg
            .must_get("hooks", &format!("priority.{name}"))
            .unwrap_or_default();

        if value.starts_with(PY_PREFIX) {
            hooks.push(Hook {
                name,
                priority,
                background: false,
                typ: HookType::Python(value.slice(PY_PREFIX.len()..)),
            });
        } else if value.starts_with(BG_PREFIX) {
            hooks.push(Hook {
                name,
                priority,
                background: true,
                typ: HookType::Shell(value.slice(BG_PREFIX.len()..)),
            });
        } else {
            hooks.push(Hook {
                name,
                priority,
                background: false,
                typ: HookType::Shell(value),
            });
        }
    }

    hooks.sort_by_key(|h| -h.priority);

    hooks
}

#[cfg(test)]
mod test {
    use std::collections::BTreeMap;

    use super::*;

    #[test]
    fn test_hooks_from_config() {
        let cfg = BTreeMap::from([
            ("hooks.foo", "echo ok"),
            ("hooks.foo.bar", "background:touch foo"),
            ("hooks.foo.baz.qux", "python:foo.py"),
            ("hooks.priority.foo.baz.qux", "1"),
            ("hooks.foobar", "echo no"),
        ]);

        let hooks = hooks_from_config(&cfg, "foo");

        assert_eq!(hooks.len(), 3);

        assert_eq!(hooks[0].name, "foo.baz.qux");
        assert!(!hooks[0].background);
        assert_eq!(hooks[0].typ, HookType::Python("foo.py".into()));

        assert_eq!(hooks[1].name, "foo");
        assert!(!hooks[1].background);
        assert_eq!(hooks[1].typ, HookType::Shell("echo ok".into()));

        assert_eq!(hooks[2].name, "foo.bar");
        assert!(hooks[2].background);
        assert_eq!(hooks[2].typ, HookType::Shell("touch foo".into()));
    }
}
