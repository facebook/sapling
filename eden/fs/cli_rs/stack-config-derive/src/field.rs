/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use darling::FromMeta;
use proc_macro2::TokenStream as TokenStream2;
use proc_macro_error::abort;
use quote::quote;
use syn::{Ident, Meta, Path};

#[derive(Debug, FromMeta)]
struct MetaArgsFlag {
    #[darling(default)]
    default: Option<()>,

    #[darling(default)]
    nested: darling::util::Flag,
}

#[derive(Debug, FromMeta)]
struct MetaArgsFunc {
    default: String,

    #[darling(default)]
    nested: darling::util::Flag,
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
}

impl ConfigField {
    pub fn new(name: Ident) -> Self {
        Self {
            name,
            default: None,
            nested: false,
        }
    }

    pub fn new_with_meta(name: Ident, meta: &Meta) -> Self {
        // We have two struct here because I can't find a way to let darling accept an optional
        // argument. i.e. `stack(default)` and `stack(default = "func")`.
        if let Ok(args) = MetaArgsFlag::from_meta(&meta) {
            let nested = args.nested.is_some();

            if args.default.is_some() && nested {
                abort!(meta, "can't use nested and default at the same time");
            }

            Self {
                name,
                default: args.default.map(|_| None),
                nested,
            }
        } else if let Ok(args) = MetaArgsFunc::from_meta(&meta) {
            let nested = args.nested.is_some();

            if nested {
                abort!(meta, "can't use nested and default at the same time");
            }

            let path = match syn::parse_str::<Path>(&args.default) {
                Ok(path) => path,
                Err(e) => abort!(meta, "default function must be a path: {}", e),
            };

            Self {
                name,
                default: Some(Some(path)),
                nested,
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
                #field: self.#field.finalize()?,
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
            quote! {
                if let Some(val) = other.#field {
                    self.#field = Some(val);
                }
            }
        }
    }
}
