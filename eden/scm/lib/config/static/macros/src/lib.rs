/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

extern crate proc_macro;
use hgrc_parser::Instruction;
use indexmap::IndexMap;
use proc_macro::TokenStream;
use proc_macro::TokenTree;

/// Generate `StaticConfig` from a static string in rc format:
///
/// ```ignore
/// static_rc!(
/// r#"
/// [section]
/// name = value
/// "#
/// )
/// ```
#[proc_macro]
pub fn static_rc(tokens: TokenStream) -> TokenStream {
    // Extract.
    let content: String = match extract_string_literal(tokens.clone()) {
        Some(content) => content,
        None => panic!(
            "static_rc requires a single string literal, got: {:?}",
            tokens
        ),
    };

    // Parse hgrc.
    let mut items: Vec<(&str, &str, Option<String>)> = Vec::new();
    for inst in hgrc_parser::parse(&content).expect("parse static_rc!") {
        match inst {
            Instruction::SetConfig {
                section,
                name,
                value,
                ..
            } => {
                items.push((section, name, Some(value.to_string())));
            }
            Instruction::UnsetConfig { section, name, .. } => {
                items.push((section, name, None));
            }
            Instruction::Include { .. } => {
                panic!("static_rc! does not support %include");
            }
        }
    }

    static_config_from_items(&items)
}

fn extract_string_literal(tokens: TokenStream) -> Option<String> {
    let mut result: Option<String> = None;
    for token in tokens {
        if result.is_some() {
            return None;
        }
        match token {
            TokenTree::Literal(lit) => {
                // Extract the string out. Note public APIs of "Literal" only provides a way to get
                // the content with surrounding " " or r#" "#. Use a naive approach to strip out
                // the " ".
                let quoted = lit.to_string();
                let content = quoted.splitn(2, '"').nth(1)?.rsplitn(2, '"').nth(1)?;
                let content = if quoted.starts_with('r') {
                    content.to_string()
                } else {
                    // Handle escapes naively.
                    content.replace(r"\n", "\n")
                };
                result = Some(content);
                continue;
            }
            TokenTree::Group(group) => {
                result = extract_string_literal(group.stream());
            }
            _ => {}
        }
    }
    result
}

/// Generate `StaticConfig` from a static string in rc format:
///
/// ```ignore
/// static_items![
///     ("section1", "name1", "value1"),
///     ("section1", "name2", "value2"),
/// ]
/// ```
#[proc_macro]
pub fn static_items(tokens: TokenStream) -> TokenStream {
    let mut items: Vec<(String, String, String)> = Vec::new();
    for token in tokens {
        if let TokenTree::Group(group) = token {
            let tokens: Vec<_> = group.stream().into_iter().collect();
            if let [section, _comma1, name, _comma2, value] = &tokens[..] {
                let section = extract_string_literal(section.clone().into()).expect("section");
                let name = extract_string_literal(name.clone().into()).expect("name");
                let value = extract_string_literal(value.clone().into()).expect("value");
                items.push((section, name, value));
            }
        }
    }
    let items: Vec<(&str, &str, Option<String>)> = items
        .iter()
        .map(|v| (v.0.as_str(), v.1.as_str(), Some(v.2.clone())))
        .collect();
    static_config_from_items(&items)
}

/// Generate code for `StaticConfig` for a list of `(section, name, value)`.
/// A `None` `value` means `%unset`. The order of the list is preserved in
/// APIs like `sections()` and `keys()`.
fn static_config_from_items(items: &[(&str, &str, Option<String>)]) -> TokenStream {
    let mut sections: IndexMap<&str, IndexMap<&str, Option<String>>> = IndexMap::new();
    for (section, name, value) in items {
        sections
            .entry(section)
            .or_default()
            .insert(name, value.clone());
    }

    // Generate code. Looks like:
    //
    // {
    //      use staticconfig::phf;
    //      // Workaround nested map. See https://github.com/rust-phf/rust-phf/issues/183
    //      const SECTION1 = phf::phf_ordered_map! {
    //          "name1" => Some("value1"),
    //          "name2" => None, // %unset
    //      };
    //      const SECTION2 = phf::phf_ordered_map! {
    //          ...
    //      };
    //      ...
    //      const SECTIONS = phf::phf_ordered_map! {
    //          "section1" => SECTION1,
    //          "section2" => SECTION2,
    //          ...
    //      };
    //      staticconfig::StaticConfig {
    //          name: "StaticConfig",
    //          sections: SECTIONS,
    //      }
    // }
    let mut code = "{ use staticconfig::phf;\n".to_string();
    for (i, (_section, items)) in sections.iter().enumerate() {
        code += &format!(
            "const SECTION{}: phf::OrderedMap<&'static str, Option<&'static str>> = phf::phf_ordered_map! {{\n",
            i
        );
        for (name, value) in items.iter() {
            code += &format!("    {:?} => {:?},\n", name, value);
        }
        code += "};\n";
    }
    code += "const SECTIONS: phf::OrderedMap<&'static str, phf::OrderedMap<&'static str, Option<&'static str>>> = phf::phf_ordered_map! {\n";
    for (i, (section, _items)) in sections.iter().enumerate() {
        code += &format!("    {:?} => SECTION{},\n", section, i);
    }
    code += "};\n";
    code += r#"staticconfig::StaticConfig::from_macro_rules(SECTIONS) }"#;
    code.parse().unwrap()
}
