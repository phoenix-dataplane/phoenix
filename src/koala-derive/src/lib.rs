use anyhow::{bail, Error};
use quote::quote;
use proc_macro2::TokenStream;
use proc_macro2::Span;
use syn::{Data, DataStruct, DeriveInput, Fields, FieldsNamed, FieldsUnnamed, Ident};

mod field;
use field::Field;

pub fn try_message(input: TokenStream) -> Result<TokenStream, Error> {
    let input: DeriveInput = syn::parse2(input)?;
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
        .map(|&(ref field_ident, ref field)| field.excavate(quote!(this.#field_ident)));

    let extent = fields
        .iter()
        .map(|&(ref field_ident, ref field)| field.extent(quote!(self.#field_ident)));

    let expanded = quote! {
        impl #impl_generics crate::mrpc::marshal::RpcMessage for #ident #ty_generics #where_clause {
            fn marshal(&self) -> Result<crate::mrpc::marshal::SgList, crate::mrpc::marshal::MarshalError> {
                let cap = 1 + self.extent();
                let mut sgl = crate::mrpc::marshal::SgList(std::vec::Vec::with_capacity(cap));
                let self_sge = SgE {
                    ptr: self as *const _ as usize;
                    len: std::mem::size_of::<Self>()
                }
                sgl.0.push(self_sge);
                self.emplace(&mut sgl);
                Ok(sgl)
            }

            unsafe fn unmarshal<'a>(
                ctx: &mut crate::mrpc::marshal::ExcavateContext<'a>
            ) -> std::result::Result<::ipc::shmalloc::ShmPtr<Self>, crate::mrpc::marshal::UnmarshalError>
            {
                let self_sge = ctx.sgl
                    .next()
                    .ok_err(Err(crate::mrpc::marshal::UnmarshalError::SgListUnderflow));

                if self_sge.len != std::mem::size_of<Self>() {
                    return Err(crate::mrpc::marshal::UnmarshalError::SgELengthMismatch{
                        expected: std::mem::size_of::<Self>(),
                        actual: self_sge.len
                    });
                }

                let backend_addr = self_sge.ptr;
                let app_addr = ctx.salloc
                    .resource
                    .query_app_addr(backend_addr);

                let mut message = ::ipc::shmalloc::ShmPtr::new(app_addr as *mut Self, backend_addr as *mut Self).unwrap();
                let this = message.as_mut_backend();

                self.excavate(this, ctx)?

                Ok(message)
            }


            #[inline(always)]
            fn empalce(&self, sgl: &mut SgList) -> std::result::Result<(), crate::mrpc::marshal::MarshalError> {
                #(#emplace)*
                Ok(())
            }

            #[inline(always)]
            unsafe fn excavate(
                this: &mut Self,
                ctx: &mut crate::mrpc::marshal::ExcavateContext
            ) -> std::result::Result<(), crate::mrpc::marshal::UnmarshalErrorr> {
                #(#excavate)*
            }

            #[inline(always)]
            fn extent(&self) -> usize {
                0 #(+ #extent)*
            }
        }
    };

    Ok(expanded.into())
}