/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

extern crate proc_macro;

use proc_macro2::TokenStream;
use quote::quote;
use syn::parse_macro_input;
use syn::Data;
use syn::DeriveInput;
use syn::Fields;
use syn::Ident;
use syn::Type;

const UNIMPLEMENTED_MSG: &str = "Only AtomicBool and AtomicI64 are supported";
const STRUCT_FIELD_MSG: &str = "Only implemented for named fields of a struct";

#[derive(Clone, PartialEq)]
enum TunableType {
    Bool,
    I64,
    String,
    VecOfStrings,
    ByRepoBool,
    ByRepoString,
    ByRepoI64,
    ByRepoVecOfStrings,
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
    fn external_type(&self) -> TokenStream {
        match self {
            Self::Bool => quote! { bool },
            Self::I64 => quote! { i64 },
            Self::String => quote! { Arc<String> },
            Self::VecOfStrings => quote! { Arc<Vec<String>> },
            Self::ByRepoBool => quote! { Option<bool> },
            Self::ByRepoString => quote! { Option<String> },
            Self::ByRepoI64 => quote! { Option<i64> },
            Self::ByRepoVecOfStrings => quote! { Option<Vec<String>> },
        }
    }

    fn by_repo_value_type(&self) -> TokenStream {
        match self {
            Self::Bool | Self::I64 | Self::String | Self::VecOfStrings => {
                panic!("Expected ByRepo flavor of tunable")
            }
            Self::ByRepoBool => quote! { bool },
            Self::ByRepoI64 => quote! { i64 },
            Self::ByRepoString => quote! { String },
            Self::ByRepoVecOfStrings => quote! { Vec<String> },
        }
    }

    fn update_container_type(&self) -> TokenStream {
        match self {
            Self::Bool => quote! { HashMap<String, bool> },
            Self::I64 => quote! { HashMap<String, i64> },
            Self::String => quote! { HashMap<String, String> },
            Self::VecOfStrings => quote! { HashMap<String, Vec<String>> },
            Self::ByRepoBool => quote! { HashMap<String, HashMap<String, bool>> },
            Self::ByRepoString => quote! { HashMap<String, HashMap<String, String>> },
            Self::ByRepoI64 => quote! { HashMap<String, HashMap<String, i64>> },
            Self::ByRepoVecOfStrings => quote! { HashMap<String, HashMap<String, Vec<String>>> },
        }
    }

    fn generate_getter_method(&self, name: Ident) -> TokenStream {
        let method = quote::format_ident!("get_{}", name);
        let by_repo_method = quote::format_ident!("get_by_repo_{}", name);

        let external_type = self.external_type();

        match &self {
            Self::Bool | Self::I64 => {
                quote! {
                    pub fn #method(&self) -> #external_type {
                        return self.#name.load(std::sync::atomic::Ordering::Relaxed)
                    }
                }
            }
            Self::String | Self::VecOfStrings => {
                quote! {
                    pub fn #method(&self) -> #external_type {
                        self.#name.load_full()
                    }
                }
            }
            Self::ByRepoBool | Self::ByRepoI64 | Self::ByRepoString | Self::ByRepoVecOfStrings => {
                quote! {
                    pub fn #by_repo_method(&self, repo: &str) -> #external_type {
                        self.#name.load_full().get(repo).map(|val| (*val).clone())
                    }
                }
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
        names_and_types.clone(),
        TunableType::I64,
        quote::format_ident!("update_ints"),
    ));

    methods.extend(generate_updater_method(
        names_and_types.clone(),
        TunableType::String,
        quote::format_ident!("update_strings"),
    ));

    methods.extend(generate_updater_method(
        names_and_types.clone(),
        TunableType::VecOfStrings,
        quote::format_ident!("update_vec_of_strings"),
    ));

    methods.extend(generate_updater_method(
        names_and_types.clone(),
        TunableType::ByRepoBool,
        quote::format_ident!("update_by_repo_bools"),
    ));

    methods.extend(generate_updater_method(
        names_and_types.clone(),
        TunableType::ByRepoString,
        quote::format_ident!("update_by_repo_strings"),
    ));

    methods.extend(generate_updater_method(
        names_and_types.clone(),
        TunableType::ByRepoI64,
        quote::format_ident!("update_by_repo_ints"),
    ));

    methods.extend(generate_updater_method(
        names_and_types,
        TunableType::ByRepoVecOfStrings,
        quote::format_ident!("update_by_repo_vec_of_strings"),
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

    let mut names = names.peekable();
    let mut body = TokenStream::new();

    if names.peek().is_some() {
        match ty {
            TunableType::I64 | TunableType::Bool => {
                body.extend(quote! {
                    #(self.#names.store(
                      tunables.get(stringify!(#names)).cloned().unwrap_or_default(),
                      std::sync::atomic::Ordering::Relaxed
                    );)*
                });
            }
            TunableType::String | TunableType::VecOfStrings => {
                body.extend(quote! {
                    #(self.#names.swap(
                      Arc::new(tunables.get(stringify!(#names)).cloned().unwrap_or_default())
                    );)*
                });
            }
            TunableType::ByRepoBool
            | TunableType::ByRepoString
            | TunableType::ByRepoI64
            | TunableType::ByRepoVecOfStrings => {
                let by_repo_value_type = ty.by_repo_value_type();
                body.extend(quote! {
                    #(
                        let mut new_values_by_repo: HashMap<String, #by_repo_value_type> = HashMap::new();
                        for (repo, val_by_tunable) in tunables {
                                for (tunable, val) in val_by_tunable {
                                    match tunable.as_ref() {
                                        stringify!(#names) => {
                                            new_values_by_repo.insert((*repo).clone(), (*val).clone());
                                        }
                                        _ => {}
                                    }
                                }
                        }
                        self.#names.swap(Arc::new(new_values_by_repo));
                    )*
                });
            }
        }
    }

    let update_container_type = ty.update_container_type();
    quote! {
        pub fn #method_name(&self, tunables: &#update_container_type) {
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
                "TunableVecOfStrings" => return TunableType::VecOfStrings,
                // TunableString is a type alias of ArcSwap<String>.
                // p.path.get_ident() returns None for ArcSwap<String>
                // and it makes it harder to parse it.
                // We use TunableString as a workaround
                "TunableString" => return TunableType::String,
                "TunableBoolByRepo" => return TunableType::ByRepoBool,
                "TunableI64ByRepo" => return TunableType::ByRepoI64,
                "TunableStringByRepo" => return TunableType::ByRepoString,
                "TunableVecOfStringsByRepo" => return TunableType::ByRepoVecOfStrings,
                _ => unimplemented!("{}, found: {}", UNIMPLEMENTED_MSG, &ident.to_string()[..]),
            }
        }
    }

    unimplemented!("{}", UNIMPLEMENTED_MSG);
}
