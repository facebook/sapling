/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use proc_macro::TokenStream;
use proc_macro_error::abort;
use proc_macro_error::proc_macro_error;
use quote::format_ident;
use quote::quote;
use syn::parse_macro_input;
use syn::parse_quote;
use syn::visit_mut;
use syn::visit_mut::VisitMut;
use syn::Attribute;
use syn::Field;
use syn::Ident;
use syn::ItemStruct;
use syn::Type;
use syn::Visibility;

mod field;

use crate::field::ConfigField;

fn opt_ident(name: &Ident) -> Ident {
    format_ident!("__StackConfig_Opt{}", name)
}

struct StackConfig {
    /// Visibility
    vis: Option<Visibility>,
    /// Struct name
    name: Ident,
    /// List of fields
    fields: Vec<ConfigField>,
}

/// Visitor related impls
impl StackConfig {
    fn take_attr(&self, attrs: &mut Vec<Attribute>) -> Option<Attribute> {
        if let Some(pos) = attrs.iter().position(|attr| {
            if let Ok(meta) = attr.parse_meta() {
                meta.path().is_ident("stack")
            } else {
                false
            }
        }) {
            Some(attrs.remove(pos))
        } else {
            None
        }
    }
}

impl VisitMut for StackConfig {
    fn visit_item_struct_mut(&mut self, item: &mut ItemStruct) {
        self.vis = Some(item.vis.clone());
        item.ident = opt_ident(&item.ident);
        visit_mut::visit_item_struct_mut(self, item);
    }

    fn visit_field_mut(&mut self, field: &mut Field) {
        let ident = match &field.ident {
            Some(ident) => ident,
            None => abort!(field, "unnamed field is not supported."),
        };

        let config_field = if let Some(attr) = self.take_attr(&mut field.attrs) {
            let meta = match attr.parse_meta() {
                Ok(meta) => meta,
                Err(e) => abort!(attr, "unable to parse attribute: {:?}", e),
            };

            ConfigField::new_with_meta(ident.clone(), &meta)
        } else {
            ConfigField::new(ident.clone())
        };

        if config_field.nested {
            if let Type::Path(path) = &mut field.ty {
                if let Some(last) = path.path.segments.last_mut() {
                    last.ident = opt_ident(&last.ident);
                }
                field.attrs.push(parse_quote! { #[serde(default)] })
            } else {
                abort!(field.ty, "can't use nested flag with this type");
            }
        } else {
            let old = &field.ty;
            field.ty = parse_quote! { Option<#old> };
        }

        self.fields.push(config_field);
        visit_mut::visit_field_mut(self, field);
    }
}

impl StackConfig {
    fn new(name: Ident) -> Self {
        Self {
            vis: None,
            name,
            fields: Vec::new(),
        }
    }

    /// Generates `std::default` implementation
    fn default_impl(&self) -> proc_macro2::TokenStream {
        let opt = opt_ident(&self.name);
        let fields = self.fields.iter().map(|field| field.default());

        quote! {
            impl ::std::default::Default for #opt {
                fn default() -> Self {
                    #opt {
                        #(#fields),*
                    }
                }
            }
        }
    }

    /// Generates conversion into the concrete type
    fn opt_impl(&self) -> proc_macro2::TokenStream {
        let name = &self.name;
        let opt = opt_ident(&self.name);
        let finalizes = self.fields.iter().map(|field| field.finalize());
        let merges = self.fields.iter().map(|field| field.merge());

        quote! {
            impl #opt {
                fn finalize(self) -> Result<#name, String> {
                    Ok(#name {
                        #(#finalizes),*
                    })
                }

                fn merge(&mut self, other: #opt) {
                    #(#merges)*
                }
            }
        }
    }

    fn builder(self) -> proc_macro2::TokenStream {
        let product = &self.name;
        let loader = format_ident!("{}Loader", self.name);
        let opt = opt_ident(&self.name);
        let vis = self.vis.unwrap_or(Visibility::Inherited);

        quote! {
            #vis struct #loader {
                product: #opt,
            }

            impl #loader {
                fn new() -> Self {
                    Self { product: Default::default() }
                }

                pub fn load(&mut self, layer: #opt) {
                    self.product.merge(layer);
                }

                pub fn build(self) -> ::std::result::Result<#product, String> {
                    self.product.finalize()
                }
            }

            impl #product {
                #vis fn loader() -> #loader {
                    #loader::new()
                }
            }
        }
    }
}

#[proc_macro_error]
#[proc_macro_derive(StackConfig, attributes(stack))]
pub fn stack_config(input: TokenStream) -> TokenStream {
    let mut input = parse_macro_input!(input as ItemStruct);

    let mut stack = StackConfig::new(input.ident.clone());
    stack.visit_item_struct_mut(&mut input);

    let opt = stack.opt_impl();
    let default = stack.default_impl();
    let build = stack.builder();

    let result = quote! {
        #[derive(stack_config::__private::Deserialize)]
        #[allow(non_camel_case_types)]
        #input

        #default
        #opt

        #build
    };

    result.into()
}
