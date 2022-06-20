use std::fmt;

use anyhow::{bail, Error};
use proc_macro2::{Span, TokenStream};
use quote::quote;
use syn::{Ident, Lit, Meta, MetaNameValue};

use crate::field::{bool_attr, set_option, tag_attr, Label};

/// A scalar protobuf field.
#[derive(Clone)]
pub struct Field {
    pub ty: Ty,
    pub kind: Kind,
    pub tag: u32,
}

impl Field {
    pub fn new(attrs: &[Meta], inferred_tag: Option<u32>) -> Result<Option<Field>, Error> {
        // default value for ty does not mean anything for our marshal/unmarshal
        // so here we ignore it, compared to how prost-derive handles it

        let mut ty = None;
        let mut label = None;
        let mut packed = None;
        let mut tag = None;

        let mut unknown_attrs = Vec::new();

        for attr in attrs {
            if let Some(t) = Ty::from_attr(attr)? {
                set_option(&mut ty, t, "duplicate type attributes")?;
            } else if let Some(p) = bool_attr("packed", attr)? {
                set_option(&mut packed, p, "duplicate packed attributes")?;
            } else if let Some(t) = tag_attr(attr)? {
                set_option(&mut tag, t, "duplicate tag attributes")?;
            } else if let Some(l) = Label::from_attr(attr) {
                set_option(&mut label, l, "duplicate label attributes")?
            } else {
                unknown_attrs.push(attr);
            }
        }

        let ty = match ty {
            Some(ty) => ty,
            None => return Ok(None),
        };

        match unknown_attrs.len() {
            0 => (),
            1 => bail!("unknown attribute: {:?}", unknown_attrs[0]),
            _ => bail!("unknown attributes: {:?}", unknown_attrs),
        }

        let tag = match tag.or(inferred_tag) {
            Some(tag) => tag,
            None => bail!("missing tag attribute"),
        };

        // repeated and packed types are treated in the same way in marshal/unmarshal
        let kind = match (label, packed) {
            (None, Some(true))
            | (Some(Label::Optional), Some(true))
            | (Some(Label::Required), Some(true)) => {
                bail!("packed attribute may only be applied to repeated fields");
            }
            (None, _) => Kind::Plain,
            (Some(Label::Optional), _) => Kind::Optional,
            (Some(Label::Required), _) => Kind::Required,
            (Some(Label::Repeated), packed) if packed.unwrap_or_else(|| ty.is_numeric()) => {
                Kind::Packed
            }
            (Some(Label::Repeated), _) => Kind::Repeated,
        };

        Ok(Some(Field { ty, kind, tag }))
    }

    pub fn emplace(&self, ident: TokenStream) -> TokenStream {
        let module = self.ty.module();

        match self.kind {
            Kind::Repeated | Kind::Packed => {
                // emplace_repeated takes in &mut Vec<$ty> and &mut SgListA
                // e.g., if a filed is repeated int32
                // the field has Rust type Vec<i32>
                // emplace_repeated dumps the buffer in this Vec<32> into a SgE
                // #ident here should be self.field_name, and it has type Vec<$ty>
                let emplace_fn = quote!(crate::mrpc::emplacement::#module::emplace_repeated);
                quote! {
                    #emplace_fn(&#ident, sgl)?;
                }
            }
            Kind::Optional => {
                if !self.ty.is_numeric() {
                    let emplace_fn = quote!(crate::mrpc::emplacement::#module::emplace_optional);
                    quote! {
                        #emplace_fn(&#ident, sgl)?;
                    }
                } else {
                    TokenStream::new()
                }
            }
            _ => {
                if !self.ty.is_numeric() {
                    let emplace_fn = quote!(crate::mrpc::emplacement::#module::emplace);
                    quote! {
                        #emplace_fn(&#ident, sgl)?;
                    }
                } else {
                    TokenStream::new()
                }
            }
        }
    }

    pub fn excavate(&self, ident: TokenStream) -> TokenStream {
        let module = self.ty.module();
        match self.kind {
            Kind::Repeated | Kind::Packed => {
                let excavate_fn = quote!(crate::mrpc::emplacement::#module::excavate_repeated);
                quote! {
                    #excavate_fn(&mut #ident, ctx)?;
                }
            }
            Kind::Optional => {
                if !self.ty.is_numeric() {
                    let excavate_fn = quote!(crate::mrpc::emplacement::#module::excavate_optional);
                    quote! {
                        #excavate_fn(&mut #ident, ctx)?;
                    }
                } else {
                    TokenStream::new()
                }
            }
            _ => {
                if !self.ty.is_numeric() {
                    let excavate_fn = quote!(crate::mrpc::emplacement::#module::excavate);
                    quote! {
                        #excavate_fn(&mut #ident, ctx)?;
                    }
                } else {
                    TokenStream::new()
                }
            }
        }
    }

    pub fn extent(&self, ident: TokenStream) -> TokenStream {
        let module = self.ty.module();

        match self.kind {
            Kind::Repeated | Kind::Packed => {
                let extent_fn = quote!(crate::mrpc::emplacement::#module::extent_repeated);
                quote! {
                    #extent_fn(&#ident)
                }
            }
            Kind::Optional => {
                if !self.ty.is_numeric() {
                    let extent_fn = quote!(crate::mrpc::emplacement::#module::extent_optional);
                    quote! {
                        #extent_fn(&#ident)
                    }
                } else {
                    quote! {
                        0
                    }
                }
            }
            _ => {
                if !self.ty.is_numeric() {
                    let extent_fn = quote!(crate::mrpc::emplacement::#module::extent);
                    quote! {
                        #extent_fn(&#ident)
                    }
                } else {
                    quote! {
                        0
                    }
                }
            }
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
pub enum Ty {
    Double,
    Float,
    Int32,
    Int64,
    Uint32,
    Uint64,
    Bool,
    String,
    Bytes,
}

impl Ty {
    pub fn from_attr(attr: &Meta) -> Result<Option<Ty>, Error> {
        let ty = match *attr {
            Meta::Path(ref name) if name.is_ident("float") => Ty::Float,
            Meta::Path(ref name) if name.is_ident("double") => Ty::Double,
            Meta::Path(ref name) if name.is_ident("int32") => Ty::Int32,
            Meta::Path(ref name) if name.is_ident("int64") => Ty::Int64,
            Meta::Path(ref name) if name.is_ident("uint32") => Ty::Uint32,
            Meta::Path(ref name) if name.is_ident("uint64") => Ty::Uint64,
            Meta::Path(ref name) if name.is_ident("bool") => Ty::Bool,
            Meta::Path(ref name) if name.is_ident("string") => Ty::String,
            Meta::Path(ref name) if name.is_ident("bytes") => Ty::Bytes,
            Meta::NameValue(MetaNameValue {
                ref path,
                lit: Lit::Str(ref l),
                ..
            }) if path.is_ident("bytes") => {
                match &l.value()[..] {
                    // we only support Vec<u8> as Rust type for bytes
                    "vec" => Ty::Bytes,
                    _ => return Ok(None),
                }
            }
            _ => return Ok(None),
        };
        Ok(Some(ty))
    }

    /// Returns the type as it appears in protobuf field declarations.
    pub fn as_str(&self) -> &'static str {
        match *self {
            Ty::Double => "double",
            Ty::Float => "float",
            Ty::Int32 => "int32",
            Ty::Int64 => "int64",
            Ty::Uint32 => "uint32",
            Ty::Uint64 => "uint64",
            Ty::Bool => "bool",
            Ty::String => "string",
            Ty::Bytes => "bytes",
        }
    }

    pub fn is_numeric(&self) -> bool {
        !matches!(self, Ty::String | Ty::Bytes)
    }

    pub fn module(&self) -> Ident {
        Ident::new(self.as_str(), Span::call_site())
    }
}

impl fmt::Debug for Ty {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl fmt::Display for Ty {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Scalar Protobuf field types.
#[derive(Clone)]
pub enum Kind {
    /// A plain proto3 scalar field.
    Plain,
    /// An optional scalar field.
    Optional,
    /// A required proto2 scalar field.
    Required,
    /// A repeated scalar field.
    Repeated,
    /// A packed repeated scalar field.
    Packed,
}
