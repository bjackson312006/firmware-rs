//! Helper procedural macros for the ADBMS6830B driver.

use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, Data, DeriveInput, Expr, Fields, ItemStruct};

/// Implements the `Register` trait for a register struct.
///
/// The `Register` trait and `RegisterKind` type are resolved at the macro use
/// site, so they must be in scope where this attribute is applied. This macro
/// does not define them.
///
/// ### Example
/// ```ignore
/// #[register_kind(RegisterKind::ReadWrite)]
/// #[bitfield(u8)]
/// pub struct ConfigA0 { ... }
/// ```
/// This generates:
/// ```ignore
/// impl Register for ConfigA0 {
///     fn kind(&self) -> RegisterKind {
///         RegisterKind::ReadWrite
///     }
/// }
/// ```
#[proc_macro_attribute]
pub fn register_kind(attr: TokenStream, item: TokenStream) -> TokenStream {
    let kind = parse_macro_input!(attr as Expr);
    let input = parse_macro_input!(item as ItemStruct);
    let name = &input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();

    quote! {
        #input

        impl #impl_generics Register for #name #ty_generics #where_clause {
            fn kind(&self) -> RegisterKind {
                #kind
            }
        }
    }
    .into()
}

/// Derives an associated `const DEFAULT: Self` for an enum, set to the variant
/// marked with `#[default]`. This exists because the standard `Default` trait isn't
/// const and can't be used in a const context.
///
/// ### Example
/// ```ignore
/// #[derive(Default, BitfieldEnumDefault)]
/// enum ComparisonThresholdVoltage {
///     Mv5_1 = 0b000,
///     #[default]
///     Mv8_1 = 0b001,
///     // ...
/// }
/// // Generates: impl ComparisonThresholdVoltage { pub const DEFAULT: Self = Self::Mv8_1; }
/// ```
#[proc_macro_derive(BitfieldEnumDefault)]
pub fn bitfield_enum_default(item: TokenStream) -> TokenStream {
    let input = parse_macro_input!(item as DeriveInput);
    let name = &input.ident;

    let Data::Enum(data) = &input.data else {
        return syn::Error::new_spanned(
            &input.ident,
            "`BitfieldEnumDefault` can only be derived for enums",
        )
        .to_compile_error()
        .into();
    };

    let mut default_variants = data
        .variants
        .iter()
        .filter(|variant| variant.attrs.iter().any(|attr| attr.path().is_ident("default")));

    let Some(default_variant) = default_variants.next() else {
        return syn::Error::new_spanned(
            name,
            "`BitfieldEnumDefault` requires exactly one variant marked with `#[default]`",
        )
        .to_compile_error()
        .into();
    };

    if let Some(extra) = default_variants.next() {
        return syn::Error::new_spanned(
            &extra.ident,
            "`BitfieldEnumDefault` requires exactly one variant marked with `#[default]`",
        )
        .to_compile_error()
        .into();
    }

    let variant_ident = &default_variant.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();

    quote! {
        impl #impl_generics #name #ty_generics #where_clause {
            pub const DEFAULT: Self = Self::#variant_ident;
        }
    }
    .into()
}

/// Derives byte serialization for a "register group" container struct whose every
/// field is a one-byte bitfield. This is useful since ADBMS6830 has a bunch of register groups
/// where every register is a single-bit bitfield. Basically, this pattern is everywhere in the datasheet
/// and deriving these manually every time would be annoying.
#[proc_macro_derive(RegisterGroup)]
pub fn register_group(item: TokenStream) -> TokenStream {
    let input = parse_macro_input!(item as DeriveInput);
    let name = &input.ident;

    let Data::Struct(data) = &input.data else {
        return syn::Error::new_spanned(
            name,
            "`RegisterGroup` can only be derived for structs with named fields",
        )
        .to_compile_error()
        .into();
    };

    let Fields::Named(fields) = &data.fields else {
        return syn::Error::new_spanned(
            name,
            "`RegisterGroup` requires named fields",
        )
        .to_compile_error()
        .into();
    };

    let field_idents: Vec<_> = fields
        .named
        .iter()
        .map(|field| field.ident.as_ref().expect("named field"))
        .collect();
    let field_types: Vec<_> = fields.named.iter().map(|field| &field.ty).collect();
    let indices: Vec<usize> = (0..field_idents.len()).collect();
    let len = field_idents.len();

    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();

    quote! {
        impl #impl_generics #name #ty_generics #where_clause {
            /// Serializes the register group into its bytes, in field declaration order.
            pub const fn to_bytes(self) -> [u8; #len] {
                [ #( self.#field_idents.into_bits() ),* ]
            }

            /// Reconstructs the register group from its bytes, in field declaration order.
            pub const fn from_bytes(bytes: [u8; #len]) -> Self {
                Self {
                    #( #field_idents: <#field_types>::from_bits(bytes[#indices]) ),*
                }
            }
        }

        impl #impl_generics ::core::convert::From<#name #ty_generics> for [u8; #len] #where_clause {
            fn from(value: #name #ty_generics) -> Self {
                value.to_bytes()
            }
        }

        impl #impl_generics ::core::convert::From<[u8; #len]> for #name #ty_generics #where_clause {
            fn from(bytes: [u8; #len]) -> Self {
                Self::from_bytes(bytes)
            }
        }
    }
    .into()
}

