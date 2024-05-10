/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

extern crate proc_macro;

use proc_macro2::Span;
use proc_macro2::TokenStream;
use quote::quote;
use syn::parse_macro_input;
use syn::spanned::Spanned;
use syn::Attribute;
use syn::Error;
use syn::Fields;
use syn::Ident;
use syn::Item;
use syn::ItemEnum;
use syn::ItemStruct;
use syn::Path;
use syn::Type;
use syn::TypePath;

#[proc_macro_derive(ThriftConvert, attributes(thrift))]
pub fn derive_thrift_convert(item: proc_macro::TokenStream) -> proc_macro::TokenStream {
    let item = parse_macro_input!(item as Item);

    match derive_thrift_convert_impl(item) {
        Ok(output) => output,
        Err(e) => e.to_compile_error(),
    }
    .into()
}

struct ThriftConvertImpl {
    name: Ident,
    thrift_type: Path,
    from_thrift: TokenStream,
    into_thrift: TokenStream,
}

fn derive_thrift_convert_impl(item: Item) -> Result<TokenStream, Error> {
    let ThriftConvertImpl {
        name,
        thrift_type,
        from_thrift,
        into_thrift,
    } = match item {
        Item::Struct(s) => derive_thrift_convert_for_struct(s)?,
        Item::Enum(e) => derive_thrift_convert_for_enum(e)?,
        _ => {
            return Err(Error::new(
                item.span(),
                "Only structs and enums are supported",
            ));
        }
    };

    Ok(quote! {
        impl ThriftConvert for #name {
            const NAME: &'static str = stringify!(#name);
            type Thrift = #thrift_type;

            #from_thrift
            #into_thrift
        }
    })
}

fn derive_thrift_convert_for_struct(s: ItemStruct) -> Result<ThriftConvertImpl, Error> {
    match &s.fields {
        Fields::Named(_) => struct_named_fields_impl(s),
        Fields::Unnamed(_) => Err(Error::new(s.span(), "Unnamed fields are not supported")),
        Fields::Unit => Err(Error::new(s.span(), "Unit structs are not supported")),
    }
}

fn derive_thrift_convert_for_enum(e: ItemEnum) -> Result<ThriftConvertImpl, Error> {
    let thrift_type: Path = find_thrift_attribute(e.span(), &e.attrs)?.parse_args()?;

    let from_thrift = enum_from_thrift(&e)?;
    let into_thrift = enum_into_thrift(&e)?;

    Ok(ThriftConvertImpl {
        name: e.ident,
        thrift_type,
        from_thrift,
        into_thrift,
    })
}

fn struct_named_fields_impl(s: ItemStruct) -> Result<ThriftConvertImpl, Error> {
    let thrift_type: Path = find_thrift_attribute(s.span(), &s.attrs)?.parse_args()?;

    let from_thrift = struct_named_fields_from_thrift(&s)?;
    let into_thrift = struct_named_fields_into_thrift(&s)?;

    Ok(ThriftConvertImpl {
        name: s.ident,
        thrift_type,
        from_thrift,
        into_thrift,
    })
}

fn struct_named_fields_from_thrift(s: &ItemStruct) -> Result<TokenStream, Error> {
    let fields = find_struct_named_fields(s)?;

    let mut from_thrift_fields = vec![];
    for (field_name, type_path) in fields {
        from_thrift_fields.push(quote! {
            #field_name: <#type_path as ThriftConvert>::from_thrift(t.#field_name)?
        });
    }

    Ok(quote! {
        fn from_thrift(t: Self::Thrift) -> anyhow::Result<Self> {
            Ok(Self {
                #(#from_thrift_fields,)*
            })
        }
    })
}

fn struct_named_fields_into_thrift(s: &ItemStruct) -> Result<TokenStream, Error> {
    let fields = find_struct_named_fields(s)?;

    let mut into_thrift_fields = vec![];
    for (field_name, type_path) in fields {
        into_thrift_fields.push(quote! {
            #field_name: <#type_path as ThriftConvert>::into_thrift(self.#field_name)
        });
    }

    Ok(quote! {
        fn into_thrift(self) -> Self::Thrift {
            Self::Thrift {
                #(#into_thrift_fields,)*
                ..Default::default()
            }
        }
    })
}

fn enum_from_thrift(e: &ItemEnum) -> Result<TokenStream, Error> {
    let thrift_type: Path = find_thrift_attribute(e.span(), &e.attrs)?.parse_args()?;

    let mut from_thrift_variants = vec![];
    for variant in &e.variants {
        let variant_ident = &variant.ident;
        let snakified_variant_ident = Ident::new(
            &snakify_pascal_case(variant_ident.to_string()),
            variant.ident.span(),
        );

        match &variant.fields {
            Fields::Unit => from_thrift_variants.push(quote! {
                #thrift_type::#snakified_variant_ident(_) => Ok(Self::#variant_ident)
            }),
            Fields::Unnamed(fields) => {
                if fields.unnamed.len() != 1 {
                    return Err(Error::new(
                        fields.span(),
                        "Exactly one unnamed field is allowed",
                    ));
                }
                let field_type = fields.unnamed.first().unwrap();
                from_thrift_variants.push(quote! {
                    #thrift_type::#snakified_variant_ident(v) => Ok(Self::#variant_ident(<#field_type as ThriftConvert>::from_thrift(v)?))
                })
            }
            Fields::Named(_) => {
                return Err(Error::new(
                    variant.span(),
                    "Named enum fields are not supported",
                ));
            }
        }
    }

    Ok(quote! {
        fn from_thrift(t: Self::Thrift) -> anyhow::Result<Self> {
            match t {
                #(#from_thrift_variants,)*
                #thrift_type::UnknownField(variant) => {
                    Err(anyhow::anyhow!("Unknown variant: {}", variant))
                }
            }
        }
    })
}

fn enum_into_thrift(e: &ItemEnum) -> Result<TokenStream, Error> {
    let thrift_type: Path = find_thrift_attribute(e.span(), &e.attrs)?.parse_args()?;

    let mut into_thrift_variants = vec![];
    for variant in &e.variants {
        let variant_ident = &variant.ident;
        let snakified_variant_ident = Ident::new(
            &snakify_pascal_case(variant_ident.to_string()),
            variant.ident.span(),
        );

        match &variant.fields {
            Fields::Unit => {
                let variant_thrift_type: Path =
                    find_thrift_attribute(variant.span(), &variant.attrs)?.parse_args()?;
                into_thrift_variants.push(quote! {
                    Self::#variant_ident => #thrift_type::#snakified_variant_ident(#variant_thrift_type {})
                })
            }
            Fields::Unnamed(fields) => {
                if fields.unnamed.len() != 1 {
                    return Err(Error::new(
                        fields.span(),
                        "Exactly one unnamed field is allowed",
                    ));
                }
                into_thrift_variants.push(quote! {
                    Self::#variant_ident(var) => #thrift_type::#snakified_variant_ident(var.into_thrift())
                })
            }
            Fields::Named(_) => {
                return Err(Error::new(
                    variant.span(),
                    "Named enum fields are not supported",
                ));
            }
        }
    }

    Ok(quote! {
        fn into_thrift(self) -> Self::Thrift {
            match self {
                #(#into_thrift_variants,)*
            }
        }
    })
}

fn find_thrift_attribute(s: Span, attrs: &[Attribute]) -> Result<&Attribute, Error> {
    for attr in attrs {
        if attr.path.is_ident("thrift") {
            return Ok(attr);
        }
    }
    Err(Error::new(
        s,
        "`thrift` attribute specifying thrift type is required to derive ThriftConvert",
    ))
}

fn find_struct_named_fields(s: &ItemStruct) -> Result<Vec<(&Ident, &TypePath)>, Error> {
    let mut fields = vec![];
    for field in s.fields.iter() {
        let field_name = field
            .ident
            .as_ref()
            .ok_or_else(|| Error::new(field.span(), "Unnamed field"))?;
        match &field.ty {
            Type::Path(path) => fields.push((field_name, path)),
            _ => return Err(Error::new(field.span(), "Unsupported field type")),
        }
    }
    Ok(fields)
}

/// Converts a Pascal case name like `SomeTraitName` to snake case like
/// `some_trait_name`.
pub(crate) fn snakify_pascal_case(pascal: impl AsRef<str>) -> String {
    let mut snake = String::new();
    for ch in pascal.as_ref().chars() {
        if ch.is_uppercase() {
            if !snake.is_empty() {
                snake.push('_');
            }
            snake.extend(ch.to_lowercase());
        } else {
            snake.push(ch);
        }
    }
    snake
}
