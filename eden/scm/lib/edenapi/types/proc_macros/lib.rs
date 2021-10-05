/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

extern crate proc_macro;

use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::spanned::Spanned;
use syn::*;

/// Derive a default implementation for a wire type for this type
// TODO: Future improvements
// - Support enums
//    - Need to add the unknown variant, implementations are slightly different
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
    let item = parse_macro_input!(item as ItemStruct);

    match get_impls(item) {
        Ok(output) => output,
        Err(e) => e.to_compile_error(),
    }
    .into()
}

fn get_impls(item: ItemStruct) -> Result<TokenStream> {
    let mut original_item = item;
    let wire_item = get_wire(&mut original_item)?;

    Ok(quote! {
        #original_item

        #wire_item
    })
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

fn get_wire(original: &mut ItemStruct) -> Result<TokenStream> {
    let mut item = original.clone();
    let ident = item.ident.clone();
    let wire_ident = format_ident!("Wire{}", ident);
    item.ident = wire_ident.clone();
    item.attrs = vec![
        parse_quote!(#[derive(Default, Debug, serde::Serialize, serde::Deserialize, Clone, PartialEq, Eq)]),
    ];

    let mut fields = vec![];

    match &mut item.fields {
        Fields::Named(ref mut fs) => fs.named.iter_mut().try_for_each(|ref mut field| {
            fields.push(field.ident.clone().unwrap());
            let (id, other_attrs): (Vec<_>, Vec<_>) = std::mem::take(&mut field.attrs)
                .into_iter()
                .partition(|attr| attr.path.is_ident("id"));
            let id = id
                .into_iter()
                .next()
                .ok_or_else(|| Error::new(field.span(), "Fields must have attribute 'id'"))?;
            let id = parse2::<Parenthesized<u16>>(id.tokens)?.0;
            field.attrs = other_attrs;
            let ty = &field.ty;
            field.ty = parse_quote!( <#ty as crate::ToWire>::Wire );
            let name = format!("{}", id);
            field.attrs.push(
                parse_quote!( #[serde(rename=#name, default, skip_serializing_if="crate::wire::is_default")] ),
            );
            Result::Ok(())
        })?,
        _ => {
            return Err(Error::new(
                item.fields.span(),
                "Only structs with named fields supported",
            ));
        }
    }

    // remove id() attribute from original struct
    match &mut original.fields {
        Fields::Named(ref mut fs) => fs.named.iter_mut().for_each(|ref mut field| {
            field.attrs = std::mem::take(&mut field.attrs)
                .into_iter()
                .filter(|attr| !attr.path.is_ident("id"))
                .collect();
        }),
        _ => unreachable!(),
    }

    let fields_to_wire = fields
        .iter()
        .map(|name| quote! { #name: self.#name.to_wire() });

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

    let fields_to_api = fields
        .iter()
        .map(|name| quote! { #name: self.#name.to_api()? });

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

    let arbitrary_impl = quote! {
        #[cfg(any(test, feature = "for-tests"))]
        impl #generics quickcheck::Arbitrary for #wire_ident #generics {
            fn arbitrary(g: &mut quickcheck::Gen) -> Self {
                use crate::ToWire;
                <Self as crate::ToApi>::Api::arbitrary(g).to_wire()
            }
        }
    };

    Ok(quote! {
        #item
        #to_wire_impl
        #to_api_impl
        #arbitrary_impl
    })
}
