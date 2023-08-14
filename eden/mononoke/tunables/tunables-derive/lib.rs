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

#[derive(Clone, PartialEq)]
enum TunableType {
    Bool,
    I64,
    U64,
    String,
    VecOfStrings,
    ByRepoBool,
    ByRepoI64,
    ByRepoU64,
    ByRepoString,
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
            Self::U64 => quote! { u64 },
            Self::String => quote! { Arc<String> },
            Self::VecOfStrings => quote! { Arc<Vec<String>> },
            Self::ByRepoBool => quote! { bool },
            Self::ByRepoI64 => quote! { i64 },
            Self::ByRepoU64 => quote! { u64 },
            Self::ByRepoString => quote! { String },
            Self::ByRepoVecOfStrings => quote! { Vec<String> },
        }
    }

    fn generate_getter_method(&self, name: Ident) -> TokenStream {
        let external_type = self.external_type();

        match &self {
            Self::Bool | Self::I64 | Self::U64 => {
                quote! {
                    pub fn #name(&self) -> Option<#external_type> {
                        self.#name.load_full().map(|arc_v| *arc_v)
                    }
                }
            }
            Self::String | Self::VecOfStrings => {
                quote! {
                    pub fn #name(&self) -> Option<#external_type> {
                        self.#name.load_full()
                    }
                }
            }
            Self::ByRepoBool
            | Self::ByRepoI64
            | Self::ByRepoU64
            | Self::ByRepoString
            | Self::ByRepoVecOfStrings => {
                let by_repo_method = quote::format_ident!("by_repo_{}", name);
                quote! {
                    pub fn #by_repo_method(&self, repo: &str) -> Option<#external_type> {
                        // If :override: has a value set for this tunable then use it, otherwise
                        // lookup the repo specific value, if not found lookup the value for :default:.
                        let values = self.#name.load_full();
                        values.get(":override:").or_else(|| values.get(repo)).or_else(|| values.get(":default:")).cloned()
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
        &[TunableType::Bool],
        quote::format_ident!("update_bools"),
        quote! { HashMap<String, bool> },
    ));

    methods.extend(generate_updater_method(
        names_and_types.clone(),
        &[TunableType::I64, TunableType::U64],
        quote::format_ident!("update_ints"),
        quote! { HashMap<String, i64> },
    ));

    methods.extend(generate_updater_method(
        names_and_types.clone(),
        &[TunableType::String],
        quote::format_ident!("update_strings"),
        quote! { HashMap<String, String> },
    ));

    methods.extend(generate_updater_method(
        names_and_types.clone(),
        &[TunableType::VecOfStrings],
        quote::format_ident!("update_vec_of_strings"),
        quote! { HashMap<String, Vec<String>> },
    ));

    methods.extend(generate_updater_method(
        names_and_types.clone(),
        &[TunableType::ByRepoBool],
        quote::format_ident!("update_by_repo_bools"),
        quote! { HashMap<String, HashMap<String, bool>> },
    ));

    methods.extend(generate_updater_method(
        names_and_types.clone(),
        &[TunableType::ByRepoI64, TunableType::ByRepoU64],
        quote::format_ident!("update_by_repo_ints"),
        quote! { HashMap<String, HashMap<String, i64>> },
    ));

    methods.extend(generate_updater_method(
        names_and_types.clone(),
        &[TunableType::ByRepoString],
        quote::format_ident!("update_by_repo_strings"),
        quote! { HashMap<String, HashMap<String, String>> },
    ));

    methods.extend(generate_updater_method(
        names_and_types,
        &[TunableType::ByRepoVecOfStrings],
        quote::format_ident!("update_by_repo_vec_of_strings"),
        quote! { HashMap<String, HashMap<String, Vec<String>>> },
    ));

    methods
}

fn generate_updater_method<I>(
    names_and_types: I,
    update_types: &[TunableType],
    method_name: Ident,
    update_container_type: TokenStream,
) -> TokenStream
where
    I: Iterator<Item = (Ident, TunableType)> + std::clone::Clone,
{
    let mut body = TokenStream::new();
    for (name, ty) in names_and_types {
        if update_types.contains(&ty) {
            match ty {
                TunableType::I64 | TunableType::U64 | TunableType::Bool => {
                    let external_type = ty.external_type();
                    body.extend(quote! {
                        self.#name.swap(new_tunables.get(stringify!(#name)).map(|v| Arc::new(*v as #external_type)));
                    });
                }
                TunableType::String | TunableType::VecOfStrings => {
                    body.extend(quote! {
                        self.#name.swap(new_tunables.get(stringify!(#name)).map(|v| Arc::new(v.clone())));
                    });
                }
                TunableType::ByRepoBool
                | TunableType::ByRepoI64
                | TunableType::ByRepoU64
                | TunableType::ByRepoString
                | TunableType::ByRepoVecOfStrings => {
                    let external_type = ty.external_type();
                    body.extend(quote! {
                        let mut new_values_by_repo: HashMap<String, _> = HashMap::new();
                        for (repo, val_by_tunable) in new_tunables {
                                if let Some(val) = val_by_tunable.get(stringify!(#name)) {
                                    new_values_by_repo.insert((*repo).clone(), (*val).clone() as #external_type);
                                }
                        }
                        self.#name.swap(Arc::new(new_values_by_repo));
                    });
                }
            }
        }
    }

    quote! {
        pub fn #method_name(&self, new_tunables: &#update_container_type) {
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
            _ => unimplemented!("Tunables requires named fields in structs"),
        },
        _ => unimplemented!("Tunables are only implemented for structs"),
    }
}

fn resolve_type(ty: Type) -> TunableType {
    // Tunables must use the tunable type aliases so that these can be parsed.
    if let Type::Path(p) = ty {
        if let Some(ident) = p.path.get_ident() {
            match &ident.to_string()[..] {
                "TunableBool" => return TunableType::Bool,
                "TunableI64" => return TunableType::I64,
                "TunableU64" => return TunableType::U64,
                "TunableVecOfStrings" => return TunableType::VecOfStrings,
                "TunableString" => return TunableType::String,
                "TunableBoolByRepo" => return TunableType::ByRepoBool,
                "TunableI64ByRepo" => return TunableType::ByRepoI64,
                "TunableU64ByRepo" => return TunableType::ByRepoU64,
                "TunableStringByRepo" => return TunableType::ByRepoString,
                "TunableVecOfStringsByRepo" => return TunableType::ByRepoVecOfStrings,
                _ => unimplemented!(
                    "Tunables type aliases must be used, found: {}",
                    &ident.to_string()[..]
                ),
            }
        }
    }

    unimplemented!("Tunables type aliases must be used")
}
