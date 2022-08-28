use anyhow::{bail, Error};
use proc_macro::TokenStream;
use proc_macro2::Span;
use quote::quote;
use syn::{
    Data, DataEnum, DataStruct, DeriveInput, Expr, Fields, FieldsNamed, FieldsUnnamed, Ident,
    Variant,
};

mod field;
use field::Field;

fn try_message(input: TokenStream) -> Result<TokenStream, Error> {
    let input: DeriveInput = syn::parse(input)?;
    let ident = input.ident;

    let variant_data = match input.data {
        Data::Struct(variant_data) => variant_data,
        Data::Enum(..) => bail!("Message can not be derived for an enum"),
        Data::Union(..) => bail!("Message can not be derived for a union"),
    };

    let generics = &input.generics;
    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();

    let fields = match variant_data {
        DataStruct {
            fields: Fields::Named(FieldsNamed { named: fields, .. }),
            ..
        }
        // tuple structs, fields are unnamed
        | DataStruct {
            fields:
            Fields::Unnamed(FieldsUnnamed {
                                unnamed: fields, ..
                            }),
            ..
        } => fields.into_iter().collect(),
        DataStruct {
            fields: Fields::Unit,
            ..
        } => Vec::new(),
    };

    let mut next_tag: u32 = 1;
    let mut fields = fields
        .into_iter()
        .enumerate()
        .flat_map(|(idx, field)| {
            let field_ident = field
                .ident
                .unwrap_or_else(|| Ident::new(&idx.to_string(), Span::call_site()));
            match Field::new(field.attrs, Some(next_tag)) {
                Ok(Some(field)) => {
                    next_tag = field.tags().iter().max().map(|t| t + 1).unwrap_or(next_tag);
                    Some(Ok((field_ident, field)))
                }
                Ok(None) => None,
                Err(err) => Some(Err(
                    err.context(format!("invalid message field {}.{}", ident, field_ident))
                )),
            }
        })
        .collect::<Result<Vec<_>, _>>()?;

    fields.sort_by_key(|&(_, ref field)| field.tags().into_iter().min().unwrap());
    let fields = fields;

    let mut tags = fields
        .iter()
        .flat_map(|&(_, ref field)| field.tags())
        .collect::<Vec<_>>();
    let num_tags = tags.len();
    tags.sort_unstable();
    tags.dedup();
    if tags.len() != num_tags {
        bail!("message {} has fields with duplicate tags", ident);
    }

    let emplace = fields
        .iter()
        .map(|&(ref field_ident, ref field)| field.emplace(quote!(self.#field_ident)));

    let excavate = fields
        .iter()
        .map(|&(ref field_ident, ref field)| field.excavate(quote!(self.#field_ident)));

    let extent = fields
        .iter()
        .map(|&(ref field_ident, ref field)| field.extent(quote!(self.#field_ident)));

    let expanded = quote! {
        impl #impl_generics ::mrpc_marshal::RpcMessage for #ident #ty_generics #where_clause {
            fn marshal(&self) -> std::result::Result<::mrpc_marshal::SgList, mrpc_marshal::MarshalError> {
                let cap = 1 + self.extent();
                let mut sgl = ::mrpc_marshal::SgList(std::vec::Vec::with_capacity(cap));
                let self_sge = ::mrpc_marshal::SgE {
                    ptr: self as *const _ as usize,
                    len: std::mem::size_of::<Self>()
                };
                sgl.0.push(self_sge);
                self.emplace(&mut sgl)?;
                Ok(sgl)
            }

            unsafe fn unmarshal<'a, A: ::mrpc_marshal::AddressArbiter>(
                ctx: &mut ::mrpc_marshal::ExcavateContext<'a, A>
            ) -> std::result::Result<::shm::ptr::ShmPtr<Self>, ::mrpc_marshal::UnmarshalError> {
                let self_sge = ctx.sgl
                    .next()
                    .ok_or(::mrpc_marshal::UnmarshalError::SgListUnderflow)?;

                if self_sge.len != std::mem::size_of::<Self>() {
                    return Err(::mrpc_marshal::UnmarshalError::SgELengthMismatch {
                        expected: std::mem::size_of::<Self>(),
                        actual: self_sge.len
                    });
                }

                let backend_addr = self_sge.ptr;
                let app_addr = ctx.addr_arbiter.query_app_addr(backend_addr)?;

                let mut message = ::shm::ptr::ShmPtr::new(app_addr as *mut Self, backend_addr as *mut Self).unwrap();
                let this = message.as_mut_backend();
                this.excavate(ctx)?;

                Ok(message)
            }


            #[inline(always)]
            fn emplace(
                &self,
                sgl: &mut ::mrpc_marshal::SgList
            ) -> std::result::Result<(), ::mrpc_marshal::MarshalError> {
                #(#emplace)*
                Ok(())
            }

            #[inline(always)]
            unsafe fn excavate<'a, A: ::mrpc_marshal::AddressArbiter>(
                &mut self,
                ctx: &mut ::mrpc_marshal::ExcavateContext<'a, A>
            ) -> std::result::Result<(), ::mrpc_marshal::UnmarshalError> {
                #(#excavate)*
                Ok(())
            }

            #[inline(always)]
            fn extent(&self) -> usize {
                0 #(+ #extent)*
            }
        }
    };

    Ok(expanded.into())
}

#[proc_macro_derive(Message, attributes(prost))]
pub fn message(input: TokenStream) -> TokenStream {
    try_message(input).unwrap()
}

fn try_enumeration(input: TokenStream) -> Result<TokenStream, Error> {
    let input: DeriveInput = syn::parse(input)?;
    let ident = input.ident;

    let generics = &input.generics;
    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();

    let punctuated_variants = match input.data {
        Data::Enum(DataEnum { variants, .. }) => variants,
        Data::Struct(_) => bail!("Enumeration can not be derived for a struct"),
        Data::Union(..) => bail!("Enumeration can not be derived for a union"),
    };

    // Map the variants into 'fields'.
    let mut variants: Vec<(Ident, Expr)> = Vec::new();
    for Variant {
        ident,
        fields,
        discriminant,
        ..
    } in punctuated_variants
    {
        match fields {
            Fields::Unit => (),
            Fields::Named(_) | Fields::Unnamed(_) => {
                bail!("Enumeration variants may not have fields")
            }
        }

        match discriminant {
            Some((_, expr)) => variants.push((ident, expr)),
            None => bail!("Enumeration variants must have a disriminant"),
        }
    }

    if variants.is_empty() {
        panic!("Enumeration must have at least one variant");
    }

    let default = variants[0].0.clone();

    let is_valid = variants
        .iter()
        .map(|&(_, ref value)| quote!(#value => true));
    let from = variants.iter().map(
        |&(ref variant, ref value)| quote!(#value => ::core::option::Option::Some(#ident::#variant)),
    );

    let is_valid_doc = format!("Returns `true` if `value` is a variant of `{}`.", ident);
    let from_i32_doc = format!(
        "Converts an `i32` to a `{}`, or `None` if `value` is not a valid variant.",
        ident
    );

    let expanded = quote! {
        impl #impl_generics #ident #ty_generics #where_clause {
            #[doc=#is_valid_doc]
            pub fn is_valid(value: i32) -> bool {
                match value {
                    #(#is_valid,)*
                    _ => false,
                }
            }

            #[doc=#from_i32_doc]
            pub fn from_i32(value: i32) -> ::core::option::Option<#ident> {
                match value {
                    #(#from,)*
                    _ => ::core::option::Option::None,
                }
            }
        }

        impl #impl_generics ::core::default::Default for #ident #ty_generics #where_clause {
            fn default() -> #ident {
                #ident::#default
            }
        }

        impl #impl_generics ::core::convert::From::<#ident> for i32 #ty_generics #where_clause {
            fn from(value: #ident) -> i32 {
                value as i32
            }
        }
    };

    Ok(expanded.into())
}

#[proc_macro_derive(Enumeration, attributes(prost))]
pub fn enumeration(input: TokenStream) -> TokenStream {
    try_enumeration(input).unwrap()
}
