use anyhow::{bail, Error};
use proc_macro2::TokenStream;
use quote::quote;
use syn::Meta;

use crate::field::{set_bool, set_option, tag_attr, word_attr, Label};

#[derive(Clone)]
pub struct Field {
    pub label: Label,
    pub tag: u32,
}

impl Field {
    pub fn new(attrs: &[Meta], inferred_tag: Option<u32>) -> Result<Option<Field>, Error> {
        let mut message = false;
        let mut label = None;
        let mut tag = None;
        let mut boxed = false;

        let mut unknown_attrs = Vec::new();

        for attr in attrs {
            if word_attr("message", attr) {
                set_bool(&mut message, "duplicate message attribute")?;
            } else if word_attr("boxed", attr) {
                set_bool(&mut boxed, "duplicate boxed attribute")?;
            } else if let Some(t) = tag_attr(attr)? {
                set_option(&mut tag, t, "duplicate tag attributes")?;
            } else if let Some(l) = Label::from_attr(attr) {
                set_option(&mut label, l, "duplicate label attributes")?;
            } else {
                unknown_attrs.push(attr);
            }
        }

        if !message {
            return Ok(None);
        }

        match unknown_attrs.len() {
            0 => (),
            1 => bail!(
                "unknown attribute for message field: {:?}",
                unknown_attrs[0]
            ),
            _ => bail!("unknown attributes for message field: {:?}", unknown_attrs),
        }

        let tag = match tag.or(inferred_tag) {
            Some(tag) => tag,
            None => bail!("message field is missing a tag attribute"),
        };

        Ok(Some(Field {
            label: label.unwrap_or(Label::Optional),
            tag,
        }))
    }

    pub fn emplace(&self, ident: TokenStream) -> TokenStream {
        match self.label {
            Label::Optional => quote! {
                crate::mrpc::emplacement::message::emplace_optional(&#ident, sgl)?;
            },
            Label::Required => quote! {
                crate::mrpc::emplacement::message::emplace(&#ident, sgl)?;
            },
            // #ident should be self.field, which is Vec<M>, M: RpcMessage
            Label::Repeated => quote! {
                crate::mrpc::emplacement::message::emplace_repeated(&#ident, sgl)?;
            },
        }
    }

    pub fn excavate(&self, ident: TokenStream) -> TokenStream {
        match self.label {
            Label::Optional => quote! {
                crate::mrpc::emplacement::message::excavate_optional(&mut #ident, ctx)?;
            },
            Label::Required => quote! {
                crate::mrpc::emplacement::message::excavate(&mut #ident, ctx)?;
            },
            Label::Repeated => quote! {
                crate::mrpc::emplacement::message::excavate_repeated(&mut #ident, ctx)?;
            },
        }
    }

    pub fn extent(&self, ident: TokenStream) -> TokenStream {
        match self.label {
            Label::Optional => quote! {
                crate::mrpc::emplacement::message::extent_optional(&#ident)
            },
            Label::Required => quote! {
                crate::mrpc::emplacement::message::extent(&#ident)
            },
            Label::Repeated => quote! {
                crate::mrpc::emplacement::message::extent_repeated(&#ident)
            },
        }
    }
}
