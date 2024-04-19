/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

extern crate proc_macro;

use std::collections::HashSet;

use proc_macro2::TokenStream;
use quote::format_ident;
use quote::quote;
use syn::spanned::Spanned;
use syn::*;

const ID: &str = "id";
const WIRE_OPTION: &str = "wire_option";

/// Derive a default implementation for a wire type for this type
/// Supports: 'wire_option' attribute. Wire impl will wrap it in Option,
//             and fail when deserializing if it's not present.
// This is useful fo safe migration off Option types in Api types.
//
// TODO: Future improvements
// - Support fields that do not implement Default on Api obj
//    - add "no_default" attribute to field. Wire impl will wrap it in Option,
//      and fail when deserializing if it's not present
// - Support generics in type
//    - Might be possible to make it work with some adaptation, at least for
//      simple "wrapper" types that just have one generic T.
#[proc_macro_attribute]
pub fn auto_wire(
    attr: proc_macro::TokenStream,
    item: proc_macro::TokenStream,
) -> proc_macro::TokenStream {
    let _ = parse_macro_input!(attr as syn::parse::Nothing);
    let mut item = parse_macro_input!(item as Item);

    let result = match &mut item {
        Item::Struct(ref mut item) => get_wire_struct(item),
        Item::Enum(ref mut item) => get_wire_enum(item),
        item => Err(Error::new(item.span(), "Only struct or enum is supported")),
    };
    match result {
        Ok(wire_item) => quote! {
            #item

            #wire_item
        },
        Err(e) => e.to_compile_error(),
    }
    .into()
}

struct Parenthesized<T>(T);
impl<T> parse::Parse for Parenthesized<T>
where
    T: std::str::FromStr,
    T::Err: std::fmt::Display,
{
    fn parse(input: parse::ParseStream) -> Result<Self> {
        let content;
        parenthesized!(content in input);
        Ok(Self(content.parse::<LitInt>()?.base10_parse::<T>()?))
    }
}

fn arbitrary_impl(wire_ident: &Ident, generics: &Generics) -> TokenStream {
    quote! {
        #[cfg(any(test, feature = "for-tests"))]
        impl #generics quickcheck::Arbitrary for #wire_ident #generics {
            fn arbitrary(g: &mut quickcheck::Gen) -> Self {
                use crate::ToWire;
                <Self as crate::ToApi>::Api::arbitrary(g).to_wire()
            }
        }
    }
}

fn extract_id(
    attrs: Vec<Attribute>,
    spanned: &impl Spanned,
    ids: &mut HashSet<u16>,
) -> Result<(u16, Vec<Attribute>)> {
    let (id, other_attrs): (Vec<_>, Vec<_>) =
        attrs.into_iter().partition(|attr| attr.path.is_ident(ID));
    if id.len() != 1 {
        return Err(Error::new(
            spanned.span(),
            "Must have exactly one attribute 'id'",
        ));
    }
    let id = id
        .into_iter()
        .next()
        // never panics because of if above
        .unwrap();
    let id = parse2::<Parenthesized<u16>>(id.tokens)?.0;
    if !ids.insert(id) {
        return Err(Error::new(
            spanned.span(),
            "'id' attribute should be unique across all fields/variants",
        ));
    }
    Ok((id, other_attrs))
}

fn remove_id(attrs: &mut Vec<Attribute>) {
    *attrs = std::mem::take(attrs)
        .into_iter()
        .filter(|attr| !attr.path.is_ident(ID))
        .collect();
}

fn extract_no_default(attrs: &mut Vec<Attribute>) -> bool {
    let mut no_default = false;
    attrs.retain(|a| match a.path.get_ident() {
        Some(id) if *id == "no_default" => {
            no_default = true;
            false
        }
        _ => true,
    });
    no_default
}

fn extract_wire_option(attrs: &mut Vec<Attribute>) -> bool {
    let mut wire_option = false;
    attrs.retain(|a| match a.path.get_ident() {
        Some(id) if *id == WIRE_OPTION => {
            wire_option = true;
            false
        }
        _ => true,
    });
    wire_option
}

fn get_wire_struct(original: &mut ItemStruct) -> Result<TokenStream> {
    let mut item = original.clone();
    let ident = item.ident.clone();
    let wire_ident = format_ident!("Wire{}", ident);
    item.ident = wire_ident.clone();

    let mut fields = vec![];
    let mut has_no_default_field = false;
    let mut wire_option_fields = HashSet::new();

    let mut ids = HashSet::new();
    match &mut item.fields {
        Fields::Named(ref mut fs) => fs.named.iter_mut().try_for_each(|ref mut field| {
            let name = field.ident.clone().unwrap();
            fields.push(name.clone());
            let (id, other_attrs) = extract_id(std::mem::take(&mut field.attrs), &field, &mut ids)?;
            field.attrs = other_attrs;
            let ty = &field.ty;
            if extract_wire_option(&mut field.attrs) {
                field.ty = parse_quote!( <Option<#ty> as crate::ToWire>::Wire );
                wire_option_fields.insert(name);
            }
            else {
                field.ty = parse_quote!( <#ty as crate::ToWire>::Wire );
            }
            let name = format!("{}", id);

            if extract_no_default(&mut field.attrs) {
                has_no_default_field = true;
                field.attrs.push(
                    parse_quote!( #[serde(rename=#name)] ),
                );
            } else {
                field.attrs.push(
                    parse_quote!( #[serde(rename=#name, default, skip_serializing_if="crate::wire::is_default")] ),
                );
            }
            Result::Ok(())
        })?,
        _ => {
            return Err(Error::new(
                item.fields.span(),
                "Only structs with named fields supported",
            ));
        }
    }

    if has_no_default_field {
        item.attrs = vec![
            parse_quote!(#[derive(Debug, serde::Serialize, serde::Deserialize, Clone, PartialEq, Eq)]),
        ];
    } else {
        item.attrs = vec![
            parse_quote!(#[derive(Default, Debug, serde::Serialize, serde::Deserialize, Clone, PartialEq, Eq)]),
        ];
    }

    // remove id() attribute from original struct
    match &mut original.fields {
        Fields::Named(ref mut fs) => fs.named.iter_mut().for_each(|ref mut field| {
            remove_id(&mut field.attrs);
            extract_no_default(&mut field.attrs);
            extract_wire_option(&mut field.attrs);
        }),
        _ => unreachable!(),
    }

    let fields_to_wire = fields.iter().map(|name| {
        if wire_option_fields.contains(name) {
            quote! { #name: Some(self.#name.to_wire()) }
        } else {
            quote! { #name: self.#name.to_wire() }
        }
    });

    let generics = &original.generics;

    let to_wire_impl = quote! {
        impl #generics crate::ToWire for #ident #generics {
            type Wire = #wire_ident #generics;

            fn to_wire(self) -> Self::Wire {
                Self::Wire {
                    #( #fields_to_wire ),*
                }
            }
        }
    };

    let fields_to_api = fields.iter().map(|name| {
        if wire_option_fields.contains(name) {
            quote! { #name: self.#name.to_api()?.ok_or_else(|| crate::WireToApiConversionError::MissingField(stringify!(#name)))? }
        } else {
            quote! { #name: self.#name.to_api()? }
        }
    });

    let to_api_impl = quote! {
        impl #generics crate::ToApi for #wire_ident #generics {
            type Api = #ident #generics;
            type Error = crate::WireToApiConversionError;

            fn to_api(self) -> Result<Self::Api, Self::Error> {
                Ok(Self::Api {
                    #( #fields_to_api ),*
                })
            }
        }
    };

    let arbitrary_impl = arbitrary_impl(&wire_ident, generics);

    Ok(quote! {
        #item
        #to_wire_impl
        #to_api_impl
        #arbitrary_impl
    })
}

fn get_wire_enum(original: &mut ItemEnum) -> Result<TokenStream> {
    // original is the original Enum
    // item is the counterpart WireEnum
    let mut item = original.clone();
    let ident = item.ident.clone();
    let wire_ident = format_ident!("Wire{}", ident);
    item.ident = wire_ident.clone();
    item.attrs = vec![
        parse_quote!(#[derive(Debug, serde::Serialize, serde::Deserialize, Clone, PartialEq, Eq)]),
    ];

    let mut variants = vec![];
    let mut ids = HashSet::new();
    item.variants.iter_mut().try_for_each(|ref mut variant| {
        let (id, other_attrs) = extract_id(std::mem::take(&mut variant.attrs), &variant, &mut ids)?;
        variant.attrs = other_attrs;
        if id == 0 {
            return Err(Error::new(variant.span(), "Variant id can't be 0"));
        }
        let name = format!("{}", id);
        variant.attrs.push(parse_quote!( #[serde(rename=#name)]));
        let unit = match &mut variant.fields {
            Fields::Unit => true,
            Fields::Unnamed(fields) if fields.unnamed.len() == 1 => {
                let field = fields.unnamed.first_mut().unwrap();
                let ty = &field.ty;
                field.ty = parse_quote!( <#ty as crate::ToWire>::Wire );
                false
            }
            _ => {
                return Err(Error::new(
                    variant.fields.span(),
                    "Only unit variants or with a single field supported",
                ));
            }
        };
        variants.push((variant.ident.clone(), unit));
        Ok(())
    })?;
    item.variants.push_value(parse_quote!(
        #[serde(other, rename = "0")]
        UnknownVariant
    ));

    // remove id() attribute from original enum
    original.variants.iter_mut().for_each(|ref mut variant| {
        remove_id(&mut variant.attrs);
    });

    let variants_to_wire = variants.iter().map(|(name, unit)| {
        if *unit {
            quote! { Self::#name => Self::Wire::#name }
        } else {
            quote! { Self::#name(value) => Self::Wire::#name(value.to_wire()) }
        }
    });

    let generics = &original.generics;

    let to_wire_impl = quote! {
        impl #generics crate::ToWire for #ident #generics {
            type Wire = #wire_ident #generics;

            fn to_wire(self) -> Self::Wire {
                match self {
                    #( #variants_to_wire ),*
                }
            }
        }
    };

    let variants_to_api = variants.iter().map(|(name, unit)| {
        if *unit {
            quote! { Self::#name => Ok(Self::Api::#name) }
        } else {
            quote! { Self::#name(value) => Ok(Self::Api::#name(value.to_api()?)) }
        }
    });

    let to_api_impl = quote! {
        impl #generics crate::ToApi for #wire_ident #generics {
            type Api = #ident #generics;
            type Error = crate::WireToApiConversionError;

            fn to_api(self) -> Result<Self::Api, Self::Error> {
                match self {
                    #( #variants_to_api, )*
                    Self::UnknownVariant => Err(Self::Error::UnrecognizedEnumVariant(stringify!(#wire_ident))),
                }
            }
        }
    };

    let arbitrary_impl = arbitrary_impl(&wire_ident, generics);

    let default_impl = quote! {
        impl #generics Default for #wire_ident #generics {
            fn default() -> Self {
                use crate::ToWire;
                #ident::default().to_wire()
            }
        }
    };

    Ok(quote! {
        #item
        #to_wire_impl
        #to_api_impl
        #arbitrary_impl
        #default_impl
    })
}

#[test]
#[should_panic = "'id' attribute should be unique across all fields/variants"]
fn test_same_ids() {
    let mut input = parse_quote! {
        enum MyEnum {
            #[id(1)]
            A,
            #[id(1)]
            B,
        }
    };
    get_wire_enum(&mut input).unwrap();
}

#[test]
#[should_panic = "Must have exactly one attribute 'id'"]
fn test_two_ids() {
    let mut input = parse_quote! {
        enum MyEnum {
            #[id(1)]
            #[id(1)]
            A,
        }
    };
    get_wire_enum(&mut input).unwrap();
}
