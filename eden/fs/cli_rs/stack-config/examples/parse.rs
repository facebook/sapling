/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use serde::Deserialize;
use stack_config::StackConfig;

#[derive(Deserialize, StackConfig, Debug, PartialEq)]
struct ConfigNested {
    field: String,
}

#[derive(Deserialize, StackConfig, Debug, PartialEq)]
struct Config {
    #[stack(default)]
    field: String,

    flag: bool,

    #[stack(default = "default_list")]
    list: Vec<String>,

    #[stack(default)]
    opt: Option<String>,

    #[stack(nested)]
    nested: ConfigNested,
}

fn default_list() -> Vec<String> {
    vec!["default".into(), "list".into()]
}

fn main() {
    let mut config_loader = Config::loader();
    config_loader.load(
        toml::from_str(
            r#"
field = "1234"

flag = false

[nested]
field = "1234"
"#,
        )
        .unwrap(),
    );
    config_loader.load(
        toml::from_str(
            r#"
flag = true

unknown = "12345"

[nested]
field = "hello"
"#,
        )
        .unwrap(),
    );

    let result = config_loader.build().unwrap();
    assert_eq!(
        result,
        Config {
            field: "1234".to_string(),
            flag: true,
            list: default_list(),
            opt: None,
            nested: ConfigNested {
                field: "hello".into()
            }
        }
    );
}
