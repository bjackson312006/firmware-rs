//! Helper procedural macros for the ADBMS6830B driver.

use proc_macro::TokenStream;
use proc_macro2::{Literal, TokenStream as TokenStream2};
use quote::{format_ident, quote};
use syn::punctuated::Punctuated;
use syn::{parse_macro_input, Data, DeriveInput, Expr, Fields, MetaNameValue, Token};

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
///     // etc!
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

/// Generates serialization code (plus some helpers) for a register group. Basically, takes a register group config struct, and creates new structs that format that register
/// group for read and write commands that can be sent directly over protocol. It handles all the PEC calculation and serialization stuff for you. Very helpful because there
/// are a lot of register groups in the ADBMS6830B and copying all these impls for each one manually would be obnoxious.
///
/// ### Using This Macro
/// You can place this macro on top of a "register group" struct. A "register group" struct is a struct whose every field is a one-byte bitfield, which follows
/// the convention from the register groups listed in the MEMORY MAP section of the ADBMS6830B datasheet (starting around page 61).
/// 
/// For a given register group, this macro generates:
/// - `to_bytes`/`from_bytes` (and matching `From` impls) for the group itself,
/// - an `impl RegisterGroup` carrying the group's `RegisterKind`,
/// - a `<Name>WriteFrame` (only if a write command is provided), a `<Name>ReadRequest`, and a
///   `<Name>ReadResponse`, each with their PEC handling generated automatically.
///
/// ### Arguments
/// This macro requires some arguments:
/// - `write`: `Some(<CommandFrame expr>)` if the group supports writing (e.g.
///   `Some(commands::config::wrcfga().frame())`), or `None` if it is read-only. When `None`, no
///   `<Name>WriteFrame` is generated and the group's `RegisterKind` is `ReadOnly`.
/// - `read`: the `<CommandFrame expr>` used to request a read (e.g.
///   `commands::config::rdcfga().frame()`).
///
/// ### Example
/// ```ignore
/// #[register_group(
///     write = Some(commands::config::wrcfga().frame()),
///     read = commands::config::rdcfga().frame(),
/// )]
/// #[derive(Clone, Copy, Debug, Default)]
/// pub struct ConfigA { stuff }
/// ```
#[proc_macro_attribute]
pub fn register_group(attr: TokenStream, item: TokenStream) -> TokenStream {
    let args =
        parse_macro_input!(attr with Punctuated::<MetaNameValue, Token![,]>::parse_terminated);
    let input = parse_macro_input!(item as DeriveInput);

    match register_group_impl(&args, input) {
        Ok(tokens) => tokens.into(),
        Err(err) => err.to_compile_error().into(),
    }
}

/// Extracts the inner `CommandFrame` expression from a `write` argument that is either
/// `Some(<expr>)` or `None`.
fn extract_write_command(expr: &Expr) -> syn::Result<Option<Expr>> {
    const MESSAGE: &str = "`write` must be `Some(<CommandFrame expr>)` or `None`";
    match expr {
        Expr::Call(call) => {
            if let Expr::Path(path) = &*call.func {
                let is_some = path
                    .path
                    .segments
                    .last()
                    .is_some_and(|segment| segment.ident == "Some");
                if is_some && call.args.len() == 1 {
                    return Ok(Some(call.args[0].clone()));
                }
            }
            Err(syn::Error::new_spanned(expr, MESSAGE))
        }
        Expr::Path(path)
            if path
                .path
                .segments
                .last()
                .is_some_and(|segment| segment.ident == "None") =>
        {
            Ok(None)
        }
        _ => Err(syn::Error::new_spanned(expr, MESSAGE)),
    }
}

fn register_group_impl(
    args: &Punctuated<MetaNameValue, Token![,]>,
    input: DeriveInput,
) -> syn::Result<TokenStream2> {
    let name = &input.ident;

    let Data::Struct(data) = &input.data else {
        return Err(syn::Error::new_spanned(
            name,
            "`register_group` can only be applied to structs with named fields",
        ));
    };
    let Fields::Named(fields) = &data.fields else {
        return Err(syn::Error::new_spanned(
            name,
            "`register_group` requires named fields",
        ));
    };

    let field_idents: Vec<_> = fields
        .named
        .iter()
        .map(|field| field.ident.as_ref().expect("named field"))
        .collect();
    let field_types: Vec<_> = fields.named.iter().map(|field| &field.ty).collect();
    let n = field_idents.len();

    // Parse the `write` and `read` arguments.
    let mut write_arg: Option<&Expr> = None;
    let mut read_arg: Option<&Expr> = None;
    for arg in args {
        if arg.path.is_ident("write") {
            write_arg = Some(&arg.value);
        } else if arg.path.is_ident("read") {
            read_arg = Some(&arg.value);
        } else {
            return Err(syn::Error::new_spanned(
                &arg.path,
                "unknown argument; expected `write` or `read`",
            ));
        }
    }
    let Some(write_arg) = write_arg else {
        return Err(syn::Error::new_spanned(
            name,
            "`register_group` requires a `write = Some(<CommandFrame>)` or `write = None` argument",
        ));
    };
    let Some(read_arg) = read_arg else {
        return Err(syn::Error::new_spanned(
            name,
            "`register_group` requires a `read = <CommandFrame>` argument",
        ));
    };
    let write_command = extract_write_command(write_arg)?;

    // Big real paths so the generated code resolves regardless of the use site's imports.
    let command_frame = quote!(crate::adbms6830b_pac::commands::CommandFrame);
    let data_pec_tx = quote!(crate::adbms6830b_pac::pec::DataPecTx);
    let data_pec_rx = quote!(crate::adbms6830b_pac::pec::DataPecRx);
    let register_kind = quote!(crate::adbms6830b_pac::registers::RegisterKind);
    let register_group_trait = quote!(crate::adbms6830b_pac::registers::RegisterGroup);

    // Calculating all the byte offsets and lenghts based on the struct schemas/field count. Not using `size_of` since i think that would include padding sometimes
    let data_indices: Vec<Literal> = (0..n).map(Literal::usize_unsuffixed).collect();
    let command_indices: Vec<Literal> = (0..4).map(Literal::usize_unsuffixed).collect();
    let group_len = Literal::usize_unsuffixed(n);
    let write_len = Literal::usize_unsuffixed(n + 6);
    let response_len = Literal::usize_unsuffixed(n + 2);
    let pec0_index = Literal::usize_unsuffixed(n);
    let pec1_index = Literal::usize_unsuffixed(n + 1);

    let write_frame_ident = format_ident!("{}WriteFrame", name);
    let read_request_ident = format_ident!("{}ReadRequest", name);
    let read_response_ident = format_ident!("{}ReadResponse", name);

    let name_str = name.to_string();
    let write_frame_str = write_frame_ident.to_string();
    let read_request_str = read_request_ident.to_string();
    let read_response_str = read_response_ident.to_string();

    let kind_value = if write_command.is_some() {
        quote!(#register_kind::ReadWrite)
    } else {
        quote!(#register_kind::ReadOnly)
    };

    let group_impl = quote! {
        impl #name {
            /// Serializes the register group into its bytes, in field declaration order.
            pub const fn to_bytes(self) -> [u8; #group_len] {
                [ #( self.#field_idents.into_bits() ),* ]
            }

            /// Reconstructs the register group from its bytes, in field declaration order.
            pub const fn from_bytes(bytes: [u8; #group_len]) -> Self {
                Self {
                    #( #field_idents: <#field_types>::from_bits(bytes[#data_indices]) ),*
                }
            }
        }

        impl ::core::convert::From<#name> for [u8; #group_len] {
            fn from(value: #name) -> Self { value.to_bytes() }
        }

        impl ::core::convert::From<[u8; #group_len]> for #name {
            fn from(bytes: [u8; #group_len]) -> Self { Self::from_bytes(bytes) }
        }

        impl #register_group_trait for #name {
            const KIND: #register_kind = #kind_value;
        }
    };

    let write_frame = if let Some(write_command_expr) = &write_command {
        let struct_doc = format!(
            "A complete protocol frame for a `{name_str}` write command.\n\nThis frame contains your `{name_str}` data plus an automatically-calculated command frame header and\ndata PEC. This frame can be constructed via `{write_frame_str}::new()`, and can be serialized into bytes (for\nsending) via `to_bytes()`."
        );
        let new_doc = format!(
            "Builds a `{write_frame_str}`.\n\n### Parameters\n- `data`: The data you want to write to this register group."
        );
        quote! {
            #[doc = #struct_doc]
            #[derive(Clone, Copy, Debug)]
            pub struct #write_frame_ident {
                command: #command_frame,
                data: #name,
                data_pec: #data_pec_tx,
            }

            impl #write_frame_ident {
                #[doc = #new_doc]
                pub const fn new(data: #name) -> Self {
                    let command = #write_command_expr;
                    let data_pec = #data_pec_tx::new(&data.to_bytes());
                    Self { command, data, data_pec }
                }

                /// Serializes the frame into bytes, for sending.
                pub const fn to_bytes(self) -> [u8; #write_len] {
                    let command = self.command.to_bytes();
                    let data = self.data.to_bytes();
                    [
                        #( command[#command_indices], )*
                        #( data[#data_indices], )*
                        self.data_pec.pec0(), self.data_pec.pec1(),
                    ]
                }

                /// The command portion of this frame.
                pub const fn command(&self) -> #command_frame { self.command }

                /// The data carried by this frame.
                pub const fn data(&self) -> #name { self.data }

                /// The data PEC computed over the provided data.
                pub const fn data_pec(&self) -> #data_pec_tx { self.data_pec }
            }
        }
    } else {
        quote! {}
    };

    let read_request_doc = format!(
        "A complete protocol frame for a `{name_str}` read request.\n\nThis frame can be transmitted to request a read of this register group. The received data can be read in\nas a `{read_response_str}`.\n\nThis frame can be constructed via `{read_request_str}::new()`, and can be serialized into bytes (for\nsending) via `to_bytes()`."
    );
    let read_request_new_doc = format!("Builds a `{read_request_str}`.");
    let read_request = quote! {
        #[doc = #read_request_doc]
        #[derive(Clone, Copy, Debug)]
        pub struct #read_request_ident {
            command: #command_frame,
        }

        impl #read_request_ident {
            #[doc = #read_request_new_doc]
            pub const fn new() -> Self {
                Self { command: #read_arg }
            }

            /// Serializes the frame into bytes, for sending.
            pub const fn to_bytes(self) -> [u8; 4] {
                self.command.to_bytes()
            }

            /// The command portion of this request.
            pub const fn command(&self) -> #command_frame { self.command }
        }

        impl ::core::default::Default for #read_request_ident {
            fn default() -> Self { Self::new() }
        }
    };

    let read_response_doc = format!(
        "A complete protocol frame for a `{name_str}` read response.\n\nThis frame can be constructed from the bytes you receive after sending a `{read_request_str}`.\n\nThis frame can be constructed via `{read_response_str}::from_bytes(bytes)`, where `bytes` are the bytes received\nfrom the device following the read request."
    );
    let read_response_from_bytes_doc = format!(
        "Parses received bytes and converts them into a `{read_response_str}`.\n\n### Returns\n- `Some({read_response_str})`, if the provided `bytes` are valid. This means your read was successful, and the register\ngroup data can be read via `data()`.\n- `None`, if the PEC check fails. This typically indicates that there was some kind of error during\nreading or transmitting the response. For more info, see the `DATA PEC` section on page 53 of the datasheet."
    );
    let read_response = quote! {
        #[doc = #read_response_doc]
        #[derive(Clone, Copy, Debug)]
        pub struct #read_response_ident {
            data: #name,
            pec: #data_pec_rx,
        }

        impl #read_response_ident {
            #[doc = #read_response_from_bytes_doc]
            pub const fn from_bytes(bytes: [u8; #response_len]) -> ::core::option::Option<Self> {
                let data = [ #( bytes[#data_indices] ),* ];
                let pec = #data_pec_rx::from_bytes([bytes[#pec0_index], bytes[#pec1_index]]);
                if !pec.verify(&data) {
                    return ::core::option::Option::None;
                }
                ::core::option::Option::Some(Self {
                    data: #name::from_bytes(data),
                    pec,
                })
            }

            /// The data carried by this frame.
            pub const fn data(&self) -> #name { self.data }

            /// The device's command counter (`CCNT[5:0]`) reported alongside this response.
            pub const fn command_counter(&self) -> u8 { self.pec.ccnt() }
        }
    };

    Ok(quote! {
        #input

        #group_impl
        #write_frame
        #read_request
        #read_response
    })
}

