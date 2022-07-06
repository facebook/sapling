/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use darling::FromMeta;
use proc_macro2::TokenStream as TokenStream2;
use proc_macro_error::abort;
use quote::quote;
use syn::Ident;
use syn::Meta;
use syn::Path;

#[derive(Debug, FromMeta)]
struct MetaArgsFlag {
    #[darling(default)]
    default: Option<()>,

    #[darling(default)]
    nested: darling::util::Flag,

    #[darling(default)]
    merge: Option<String>,
}

#[derive(Debug, FromMeta)]
struct MetaArgsFunc {
    default: String,

    #[darling(default)]
    nested: darling::util::Flag,

    #[darling(default)]
    merge: Option<String>,
}

pub struct ConfigField {
    pub name: Ident,

    /// Specify where to look for default value
    ///
    /// None - no default
    /// Some(None) - use std::default::Default
    /// Some(path) - use path() as default
    pub default: Option<Option<Path>>,

    pub nested: bool,

    pub merge: Option<Path>,
}

impl ConfigField {
    pub fn new(name: Ident) -> Self {
        Self {
            name,
            default: None,
            nested: false,
            merge: None,
        }
    }

    fn to_path(meta: &Meta, field: &str, st: &str) -> Path {
        match syn::parse_str::<Path>(st) {
            Ok(path) => path,
            Err(e) => abort!(meta, "{} must be a path: {}", field, e),
        }
    }

    pub fn new_with_meta(name: Ident, meta: &Meta) -> Self {
        // We have two struct here because I can't find a way to let darling accept an optional
        // argument. i.e. `stack(default)` and `stack(default = "func")`.
        if let Ok(args) = MetaArgsFlag::from_meta(&meta) {
            let nested = args.nested.is_present();

            if args.default.is_some() && nested {
                abort!(meta, "can't use nested and default at the same time");
            }

            if args.merge.is_some() && nested {
                abort!(meta, "can't use nested and merge at the same time");
            }

            let merge = args.merge.map(|x| Self::to_path(meta, "merge", &x));

            Self {
                name,
                default: args.default.map(|_| None),
                nested,
                merge,
            }
        } else if let Ok(args) = MetaArgsFunc::from_meta(&meta) {
            let nested = args.nested.is_present();

            if nested {
                abort!(meta, "can't use nested and default at the same time");
            }

            let merge = args.merge.map(|x| Self::to_path(meta, "merge", &x));

            Self {
                name,
                default: Some(Some(Self::to_path(meta, "default", &args.default))),
                nested,
                merge,
            }
        } else {
            abort!(meta, "uanble to parse attribute");
        }
    }

    pub fn default(&self) -> TokenStream2 {
        let field = &self.name;
        if let Some(default) = &self.default {
            if let Some(func) = default {
                // Default attribute with user-defined function call
                quote! {
                    #field: Some(#func())
                }
            } else {
                // Default attribute with no parameter, use `Default::default`
                quote! {
                    #field: Some(::std::default::Default::default())
                }
            }
        } else if self.nested {
            quote! {
                #field: std::default::Default::default()
            }
        } else {
            // No default supplied, do nothing
            quote! {
                #field: None
            }
        }
    }

    pub fn finalize(&self) -> TokenStream2 {
        let field = &self.name;
        let field_str = field.to_string();

        if self.nested {
            quote! {
                #field: self.#field.finalize()?
            }
        } else {
            quote! {
                #field: self.#field.ok_or_else(|| format!("field {} is missing", #field_str))?
            }
        }
    }

    pub fn merge(&self) -> TokenStream2 {
        let field = &self.name;

        if self.nested {
            quote! {
                self.#field.merge(other.#field);
            }
        } else {
            if let Some(merge) = &self.merge {
                quote! {
                    if let Some(val) = other.#field {
                        if let Some(current) = self.#field.as_mut() {
                            #merge(current, val);
                        } else {
                            self.#field = Some(val);
                        }
                    }
                }
            } else {
                quote! {
                    if let Some(val) = other.#field {
                        self.#field = Some(val);
                    }
                }
            }
        }
    }
}
