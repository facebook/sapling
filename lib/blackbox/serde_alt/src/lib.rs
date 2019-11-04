/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

extern crate proc_macro;
use proc_macro::{Group, Ident, TokenStream, TokenTree};

/// Define an alternative structure (or enum) ending with `Alt`, with serde
/// attribute "alias" and "rename" swapped.
#[proc_macro_attribute]
pub fn serde_alt(_attr: TokenStream, item: TokenStream) -> TokenStream {
    fn translate(stream: TokenStream) -> TokenStream {
        let mut result = TokenStream::new();
        for tt in stream {
            let new_tt: TokenTree = match tt {
                TokenTree::Group(group) => {
                    let new_stream = translate(group.stream());
                    let new_group = Group::new(group.delimiter(), new_stream);
                    new_group.into()
                }
                TokenTree::Ident(id) => {
                    let name = id.to_string();
                    let new_name = if name.chars().nth(0).unwrap().is_uppercase()
                        && name != "BTreeMap"
                        && name != "Clone"
                        && name != "Copy"
                        && name != "Debug"
                        && name != "Default"
                        && name != "Deserialize"
                        && name != "Display"
                        && name != "Eq"
                        && name != "Formatter"
                        && name != "HashMap"
                        && name != "Option"
                        && name != "Ord"
                        && name != "PartialEq"
                        && name != "Result"
                        && name != "Serialize"
                        && name != "String"
                        && name != "Value"
                        && name != "Vec"
                    {
                        // Append "Alt" to the type name
                        format!("{}Alt", name)
                    } else if name == "alias" {
                        // Translate "alias" to "rename"
                        "rename".to_string()
                    } else if name == "rename" {
                        // Translate "rename" to "alias"
                        "alias".to_string()
                    } else {
                        // Unchanged (ex. "u32", "String", field names)
                        name
                    };
                    Ident::new(&new_name, id.span()).into()
                }
                _ => tt.clone().into(),
            };
            let new_stream: TokenStream = new_tt.into();
            result.extend(new_stream);
        }
        result
    }

    let mut new_stream = translate(item.clone());
    new_stream.extend(item);
    new_stream
}
