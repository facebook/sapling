/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under both the MIT license found in the
 * LICENSE-MIT file in the root directory of this source tree and the Apache
 * License, Version 2.0 found in the LICENSE-APACHE file in the root directory
 * of this source tree.
 */

//! A proc-macro to auto-derive the [`quickcheck::Arbitrary`](https://docs.rs/quickcheck/latest/quickcheck/trait.Arbitrary.html) trait.

use quote::quote;
use quote::quote_spanned;
use syn::DeriveInput;
use syn::spanned::Spanned;

/// Derive the [`quickcheck::Arbitrary`](https://docs.rs/quickcheck/latest/quickcheck/trait.Arbitrary.html) trait.
///
/// This macro supports structs, tuple structs, unit types and enums. For implementing the `quickcheck::Arbitrary::arbitrary`
/// method, it calls `quickcheck::Arbitrary::arbitrary` for each of the contained fields and uniformly samples the variants of each enum.
/// The `quickcheck::Arbitrary::shrink` method uses the default implementation for now, which returns an empty boxed iterator.
///
/// At compile-time, this macro prevents implementing the trait for enums with no variants (never types or void types)
/// and for untagged unions. For untagged unions, in particular, it is dangerous to have arbitrary variants generated since they
/// can only be used safely if the exact variant they hold is known.
///
/// ## Comprehensive example
///
/// ```
/// use quickcheck_arbitrary_derive::Arbitrary;
///
/// #[derive(Arbitrary, Clone, Debug)]
/// struct StructFoo {
///     bar: u8,
///     baz: String,
/// }
///
/// #[derive(Arbitrary, Clone, Debug)]
/// struct UnitFoo;
///
/// #[derive(Arbitrary, Clone, Debug)]
/// struct TupleFoo(u8, String);
///
/// #[derive(Arbitrary, Clone, Debug)]
/// enum EnumFoo {
///     Foo { foo: StructFoo, bar: Vec<u8> },
///     Bar { hello: i64 },
///     Baz(u8),
///     Qux,
/// }
///
/// use quickcheck::Arbitrary;
/// use quickcheck::Gen;
///
/// let mut random = Gen::new(10);
/// println!("{:#?}", StructFoo::arbitrary(&mut random));
/// println!("{:#?}", TupleFoo::arbitrary(&mut random));
/// println!("{:#?}", UnitFoo::arbitrary(&mut random));
/// println!("{:#?}", EnumFoo::arbitrary(&mut random));
/// ```
#[proc_macro_derive(Arbitrary)]
pub fn derive(input: proc_macro::TokenStream) -> proc_macro::TokenStream {
    derive_arbitrary(input.into()).into()
}

const EMPTY_ENUMS_DISALLOWED: &str =
    "Enums that have no variants have no values, so they cannot be generated.";
const UNTAGGED_UNIONS_DISALLOWED: &str = "Untagged unions can only be read if we know which kind of data they contain. This is why arbitrarily generated untagged unions are at best useless and could also be dangerous.";

fn derive_arbitrary(input: proc_macro2::TokenStream) -> proc_macro2::TokenStream {
    let DeriveInput {
        ident,
        generics,
        data,
        ..
    } = match syn::parse2(input) {
        Ok(input) => input,
        Err(err) => return err.to_compile_error(),
    };

    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();

    let implem = match data {
        syn::Data::Struct(data) => {
            let fields = generate_arbitrary_fields(data.fields);
            quote! { Self #fields }
        }
        syn::Data::Enum(data) => {
            if data.variants.is_empty() {
                return syn::Error::new(ident.span(), EMPTY_ENUMS_DISALLOWED).into_compile_error();
            }
            let arbitraries = data.variants.into_iter().map(|variant| {
                let variant_ident = variant.ident;
                let variant_fields = generate_arbitrary_fields(variant.fields);
                quote! { Self :: #variant_ident #variant_fields }
            });

            let variant_indices = (0..arbitraries.len()).map(syn::Index::from);
            let num_variants = syn::Index::from(arbitraries.len());
            quote! {
                match g.choose((0..#num_variants).collect::<Vec<_>>().as_slice()) {
                    #(Some(&#variant_indices) => #arbitraries,)*
                    _ => unreachable!("You encountered a bug within the `quickcheck_arbitrary_derive` crate. Please report it back to the maintainers. Thank you! :)"),
                }
            }
        }
        syn::Data::Union(_) => {
            return syn::Error::new(ident.span(), UNTAGGED_UNIONS_DISALLOWED).into_compile_error();
        }
    };

    quote! {
        impl #impl_generics quickcheck::Arbitrary for #ident #ty_generics #where_clause {
            fn arbitrary(g: &mut quickcheck::Gen) -> Self {
                #implem
            }
        }
    }
}

fn generate_arbitrary_fields(fields: syn::Fields) -> proc_macro2::TokenStream {
    match fields {
        syn::Fields::Named(fields) => {
            let arbitraries = fields.named.into_iter().map(|field| {
                let field_span = field.span();
                let ident = field.ident;
                quote_spanned! {field_span=> #ident: quickcheck::Arbitrary::arbitrary(g)}
            });
            quote! { { #(#arbitraries,)* } }
        }
        syn::Fields::Unnamed(fields) => {
            let arbitraries = fields
                .unnamed
                .into_iter()
                .map(|field| quote_spanned! {field.span()=> quickcheck::Arbitrary::arbitrary(g)});
            quote! { (#(#arbitraries,)*) }
        }
        syn::Fields::Unit => quote! {},
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn struct_named_fields() {
        let input = quote! {
            #[derive(Arbitrary)]
            struct Foo {
                bar: u8,
                baz: String,
            }
        };

        let output = quote! {
            impl quickcheck::Arbitrary for Foo {
                fn arbitrary(g: &mut quickcheck::Gen) -> Self {
                    Self {
                        bar: quickcheck::Arbitrary::arbitrary(g),
                        baz: quickcheck::Arbitrary::arbitrary(g),
                    }
                }
            }
        };

        assert_eq!(derive_arbitrary(input).to_string(), output.to_string());
    }

    #[test]
    fn struct_tuple() {
        let input = quote! {
            #[derive(Arbitrary)]
            struct Foo(u8, String);
        };

        let output = quote! {
            impl quickcheck::Arbitrary for Foo {
                fn arbitrary(g: &mut quickcheck::Gen) -> Self {
                    Self(
                        quickcheck::Arbitrary::arbitrary(g),
                        quickcheck::Arbitrary::arbitrary(g),
                    )
                }
            }
        };

        assert_eq!(derive_arbitrary(input).to_string(), output.to_string());
    }

    #[test]
    fn struct_unit() {
        let input = quote! {
            #[derive(Arbitrary)]
            struct Foo;
        };

        let output = quote! {
            impl quickcheck::Arbitrary for Foo {
                fn arbitrary(g: &mut quickcheck::Gen) -> Self {
                    Self
                }
            }
        };

        assert_eq!(derive_arbitrary(input).to_string(), output.to_string());
    }

    #[test]
    fn enum_all() {
        let input = quote! {
            #[derive(Arbitrary)]
            enum Foo {
                Foo {foo: String, bar: Vec<u8> },
                Bar { hello: i64 },
                Baz(u8),
                Qux,
            }
        };

        let output = quote! {
            impl quickcheck::Arbitrary for Foo {
                fn arbitrary(g: &mut quickcheck::Gen) -> Self {
                    match g.choose((0..4).collect:: <Vec<_>>().as_slice()) {
                        Some(&0) => Self::Foo{foo: quickcheck::Arbitrary::arbitrary(g), bar: quickcheck::Arbitrary::arbitrary(g),},
                        Some(&1) => Self::Bar{hello: quickcheck::Arbitrary::arbitrary(g),},
                        Some(&2) => Self::Baz(quickcheck::Arbitrary::arbitrary(g),),
                        Some(&3) => Self::Qux,
                        _ => unreachable!("You encountered a bug within the `quickcheck_arbitrary_derive` crate. Please report it back to the maintainers. Thank you! :)"),
                    }
                }
            }
        };

        assert_eq!(derive_arbitrary(input).to_string(), output.to_string());
    }

    #[test]
    fn enum_never() {
        let input = quote! {
            #[derive(Arbitrary)]
            enum Foo{}
        };

        let output = quote! {
            ::core::compile_error! { #EMPTY_ENUMS_DISALLOWED }
        };

        assert_eq!(derive_arbitrary(input).to_string(), output.to_string());
    }

    #[test]
    fn untagged_union() {
        let input = quote! {
            #[derive(Arbitrary)]
            #[repr(C)]
            union Foo {
                foo: i32,
                bar: i32,
                baz: i32,
            }
        };

        let output = quote! {
            ::core::compile_error! { #UNTAGGED_UNIONS_DISALLOWED }
        };

        assert_eq!(derive_arbitrary(input).to_string(), output.to_string());
    }
}
