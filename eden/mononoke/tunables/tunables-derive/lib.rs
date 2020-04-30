/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

extern crate proc_macro;

use proc_macro2::TokenStream;
use quote::quote;
use syn::{parse_macro_input, Data, DeriveInput, Fields, Ident, Type};

const UNIMPLEMENTED_MSG: &str = "Only AtomicBool and AtomicI64 are supported";
const STRUCT_FIELD_MSG: &str = "Only implemented for named fields of a struct";

#[derive(Clone, PartialEq)]
enum TunableType {
    Bool,
    I64,
}

#[proc_macro_derive(Tunables)]
// This proc macro accepts a struct and provides methods that get the atomic
// values stored inside of it. It does this by generating methods
// named get_<field>(). The macro also generates methods that update the
// atomic values inside of the struct, using a provided HashMap.
pub fn derive_tunables(input: proc_macro::TokenStream) -> proc_macro::TokenStream {
    let parsed_input = parse_macro_input!(input as DeriveInput);

    let struct_name = parsed_input.ident;
    let names_and_types = parse_names_and_types(parsed_input.data).into_iter();

    let getter_methods = generate_getter_methods(names_and_types.clone());
    let updater_methods = generate_updater_methods(names_and_types);

    let expanded = quote! {
        impl #struct_name {
            #updater_methods
            #getter_methods
        }
    };

    expanded.into()
}

impl TunableType {
    fn external_type(&self) -> Ident {
        match self {
            Self::Bool => quote::format_ident!("{}", "bool"),
            Self::I64 => quote::format_ident!("{}", "i64"),
        }
    }

    fn generate_getter_method(&self, name: Ident) -> TokenStream {
        let method = quote::format_ident!("get_{}", name);
        let external_type = self.external_type();

        quote! {
            pub fn #method(&self) -> #external_type {
                return self.#name.load(std::sync::atomic::Ordering::Relaxed)
            }
        }
    }
}

fn generate_getter_methods<I>(names_and_types: I) -> TokenStream
where
    I: Iterator<Item = (Ident, TunableType)> + std::clone::Clone,
{
    let mut methods = TokenStream::new();

    for (name, ty) in names_and_types {
        methods.extend(ty.generate_getter_method(name));
    }

    methods
}

fn generate_updater_methods<I>(names_and_types: I) -> TokenStream
where
    I: Iterator<Item = (Ident, TunableType)> + std::clone::Clone,
{
    let mut methods = TokenStream::new();

    methods.extend(generate_updater_method(
        names_and_types.clone(),
        TunableType::Bool,
        quote::format_ident!("update_bools"),
    ));

    methods.extend(generate_updater_method(
        names_and_types,
        TunableType::I64,
        quote::format_ident!("update_ints"),
    ));

    methods
}

fn generate_updater_method<I>(
    names_and_types: I,
    ty: TunableType,
    method_name: Ident,
) -> TokenStream
where
    I: Iterator<Item = (Ident, TunableType)> + std::clone::Clone,
{
    let names = names_and_types.filter(|(_, t)| *t == ty).map(|(n, _)| n);

    let type_ident = ty.external_type();
    let mut names = names.peekable();
    let mut body = TokenStream::new();

    if names.peek().is_some() {
        body.extend(
            quote! {
                for (name, val) in tunables.iter() {
                    match name.as_ref() {
                        #(stringify!(#names) => self.#names.store(*val, std::sync::atomic::Ordering::Relaxed), )*
                        _ => {}
                    }
                }
            }
        )
    }

    quote! {
        fn #method_name(&self, tunables: &std::collections::HashMap<String, #type_ident>) {
            #body
        }
    }
}

fn parse_names_and_types(data: Data) -> Vec<(Ident, TunableType)> {
    match data {
        Data::Struct(data) => match data.fields {
            Fields::Named(fields) => fields
                .named
                .into_iter()
                .filter_map(|f| f.clone().ident.map(|i| (i, resolve_type(f.ty))))
                .collect::<Vec<_>>(),
            _ => unimplemented!("{}", STRUCT_FIELD_MSG),
        },
        _ => unimplemented!("{}", STRUCT_FIELD_MSG),
    }
}

fn resolve_type(ty: Type) -> TunableType {
    // TODO: Handle full paths to the types, such as
    // std::sync::atomic::AtomicBool, rather than just the type name.
    if let Type::Path(p) = ty {
        if let Some(ident) = p.path.get_ident() {
            match &ident.to_string()[..] {
                "AtomicBool" => return TunableType::Bool,
                "AtomicI64" => return TunableType::I64,
                _ => unimplemented!("{}", UNIMPLEMENTED_MSG),
            }
        }
    }

    unimplemented!("{}", UNIMPLEMENTED_MSG);
}
