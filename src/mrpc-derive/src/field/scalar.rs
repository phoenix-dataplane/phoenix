use std::fmt;

use anyhow::{bail, Error};
use proc_macro2::{Span, TokenStream};
use quote::{quote, ToTokens, TokenStreamExt};
use syn::{parse_str, Ident, Lit, LitByteStr, Meta, MetaList, MetaNameValue, NestedMeta, Path};

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
        let mut default = None;
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
            } else if let Some(d) = DefaultValue::from_attr(attr)? {
                set_option(&mut default, d, "duplicate default attributes")?;
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

        let has_default = default.is_some();
        let default = default.map_or_else(
            || Ok(DefaultValue::new(&ty)),
            |lit| DefaultValue::from_lit(&ty, lit),
        )?;

        // repeated and packed types are treated in the same way in marshal/unmarshal
        let kind = match (label, packed, has_default) {
            (None, Some(true), _)
            | (Some(Label::Optional), Some(true), _)
            | (Some(Label::Required), Some(true), _) => {
                bail!("packed attribute may only be applied to repeated fields");
            }
            (Some(Label::Repeated), Some(true), _) if !ty.is_numeric() => {
                bail!("packed attribute may only be applied to numeric types");
            }
            (Some(Label::Repeated), _, true) => {
                bail!("repeated fields may not have a default value");
            }

            (None, _, _) => Kind::Plain(default),
            (Some(Label::Optional), _, _) => Kind::Optional(default),
            (Some(Label::Required), _, _) => Kind::Required(default),
            (Some(Label::Repeated), packed, false) if packed.unwrap_or_else(|| ty.is_numeric()) => {
                Kind::Packed
            }
            (Some(Label::Repeated), _, false) => Kind::Repeated,
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
                let emplace_fn = quote!(::mrpc_marshal::emplacement::#module::emplace_repeated);
                quote! {
                    #emplace_fn(&#ident, sgl)?;
                }
            }
            Kind::Optional(_) => {
                if !self.ty.is_numeric() {
                    let emplace_fn = quote!(::mrpc_marshal::emplacement::#module::emplace_optional);
                    quote! {
                        #emplace_fn(&#ident, sgl)?;
                    }
                } else {
                    TokenStream::new()
                }
            }
            _ => {
                if !self.ty.is_numeric() {
                    let emplace_fn = quote!(::mrpc_marshal::emplacement::#module::emplace);
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
                let excavate_fn = quote!(::mrpc_marshal::emplacement::#module::excavate_repeated);
                quote! {
                    #excavate_fn(&mut #ident, ctx)?;
                }
            }
            Kind::Optional(_) => {
                if !self.ty.is_numeric() {
                    let excavate_fn =
                        quote!(::mrpc_marshal::emplacement::#module::excavate_optional);
                    quote! {
                        #excavate_fn(&mut #ident, ctx)?;
                    }
                } else {
                    TokenStream::new()
                }
            }
            _ => {
                if !self.ty.is_numeric() {
                    let excavate_fn = quote!(::mrpc_marshal::emplacement::#module::excavate);
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
                let extent_fn = quote!(::mrpc_marshal::emplacement::#module::extent_repeated);
                quote! {
                    #extent_fn(&#ident)
                }
            }
            Kind::Optional(_) => {
                if !self.ty.is_numeric() {
                    let extent_fn = quote!(::mrpc_marshal::emplacement::#module::extent_optional);
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
                    let extent_fn = quote!(::mrpc_marshal::emplacement::#module::extent);
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
    Sint32,
    Sint64,
    Fixed32,
    Fixed64,
    Sfixed32,
    Sfixed64,
    Bool,
    String,
    Bytes(BytesTy),
    Enumeration(Path),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BytesTy {
    Vec,
    #[allow(unused)]
    Bytes,
}

impl BytesTy {
    fn try_from_str(s: &str) -> Result<Self, Error> {
        match s {
            "vec" => Ok(BytesTy::Vec),
            "bytes" => bail!("TODO: add support for bytes"),
            _ => bail!("Invalid bytes type: {}", s),
        }
    }
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
            Meta::Path(ref name) if name.is_ident("sint32") => Ty::Sint32,
            Meta::Path(ref name) if name.is_ident("sint64") => Ty::Sint64,
            Meta::Path(ref name) if name.is_ident("fixed32") => Ty::Fixed32,
            Meta::Path(ref name) if name.is_ident("fixed64") => Ty::Fixed64,
            Meta::Path(ref name) if name.is_ident("sfixed32") => Ty::Sfixed32,
            Meta::Path(ref name) if name.is_ident("sfixed64") => Ty::Sfixed64,
            Meta::Path(ref name) if name.is_ident("bool") => Ty::Bool,
            Meta::Path(ref name) if name.is_ident("string") => Ty::String,
            Meta::Path(ref name) if name.is_ident("bytes") => Ty::Bytes(BytesTy::Vec),
            Meta::NameValue(MetaNameValue {
                ref path,
                lit: Lit::Str(ref l),
                ..
            }) if path.is_ident("bytes") => Ty::Bytes(BytesTy::try_from_str(&l.value())?),
            Meta::NameValue(MetaNameValue {
                ref path,
                lit: Lit::Str(ref l),
                ..
            }) if path.is_ident("enumeration") => Ty::Enumeration(parse_str::<Path>(&l.value())?),
            Meta::List(MetaList {
                ref path,
                ref nested,
                ..
            }) if path.is_ident("enumeration") => {
                // TODO(rustlang/rust#23121): slice pattern matching would make this much nicer.
                if nested.len() == 1 {
                    if let NestedMeta::Meta(Meta::Path(ref path)) = nested[0] {
                        Ty::Enumeration(path.clone())
                    } else {
                        bail!("invalid enumeration attribute: item must be an identifier");
                    }
                } else {
                    bail!("invalid enumeration attribute: only a single identifier is supported");
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
            Ty::Sint32 => "sint32",
            Ty::Sint64 => "sint64",
            Ty::Fixed32 => "fixed32",
            Ty::Fixed64 => "fixed64",
            Ty::Sfixed32 => "sfixed32",
            Ty::Sfixed64 => "sfixed64",
            Ty::Bool => "bool",
            Ty::String => "string",
            Ty::Bytes(..) => "bytes",
            Ty::Enumeration(..) => "enum",
        }
    }

    pub fn is_numeric(&self) -> bool {
        !matches!(self, Ty::String | Ty::Bytes(..))
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
    Plain(DefaultValue),
    /// An optional scalar field.
    Optional(DefaultValue),
    /// A required proto2 scalar field.
    Required(DefaultValue),
    /// A repeated scalar field.
    Repeated,
    /// A packed repeated scalar field.
    Packed,
}

/// Scalar Protobuf field default value.
#[derive(Clone, Debug)]
pub enum DefaultValue {
    F64(f64),
    F32(f32),
    I32(i32),
    I64(i64),
    U32(u32),
    U64(u64),
    Bool(bool),
    String(String),
    Bytes(Vec<u8>),
    Enumeration(TokenStream),
    Path(Path),
}

impl DefaultValue {
    pub fn from_attr(attr: &Meta) -> Result<Option<Lit>, Error> {
        if !attr.path().is_ident("default") {
            Ok(None)
        } else if let Meta::NameValue(ref name_value) = *attr {
            Ok(Some(name_value.lit.clone()))
        } else {
            bail!("invalid default value attribute: {:?}", attr)
        }
    }

    pub fn from_lit(ty: &Ty, lit: Lit) -> Result<DefaultValue, Error> {
        let is_i32 = *ty == Ty::Int32 || *ty == Ty::Sint32 || *ty == Ty::Sfixed32;
        let is_i64 = *ty == Ty::Int64 || *ty == Ty::Sint64 || *ty == Ty::Sfixed64;

        let is_u32 = *ty == Ty::Uint32 || *ty == Ty::Fixed32;
        let is_u64 = *ty == Ty::Uint64 || *ty == Ty::Fixed64;

        let empty_or_is = |expected, actual: &str| expected == actual || actual.is_empty();

        let default = match lit {
            Lit::Int(ref lit) if is_i32 && empty_or_is("i32", lit.suffix()) => {
                DefaultValue::I32(lit.base10_parse()?)
            }
            Lit::Int(ref lit) if is_i64 && empty_or_is("i64", lit.suffix()) => {
                DefaultValue::I64(lit.base10_parse()?)
            }
            Lit::Int(ref lit) if is_u32 && empty_or_is("u32", lit.suffix()) => {
                DefaultValue::U32(lit.base10_parse()?)
            }
            Lit::Int(ref lit) if is_u64 && empty_or_is("u64", lit.suffix()) => {
                DefaultValue::U64(lit.base10_parse()?)
            }

            Lit::Float(ref lit) if *ty == Ty::Float && empty_or_is("f32", lit.suffix()) => {
                DefaultValue::F32(lit.base10_parse()?)
            }
            Lit::Int(ref lit) if *ty == Ty::Float => DefaultValue::F32(lit.base10_parse()?),

            Lit::Float(ref lit) if *ty == Ty::Double && empty_or_is("f64", lit.suffix()) => {
                DefaultValue::F64(lit.base10_parse()?)
            }
            Lit::Int(ref lit) if *ty == Ty::Double => DefaultValue::F64(lit.base10_parse()?),

            Lit::Bool(ref lit) if *ty == Ty::Bool => DefaultValue::Bool(lit.value),
            Lit::Str(ref lit) if *ty == Ty::String => DefaultValue::String(lit.value()),
            Lit::ByteStr(ref lit)
                if *ty == Ty::Bytes(BytesTy::Bytes) || *ty == Ty::Bytes(BytesTy::Vec) =>
            {
                DefaultValue::Bytes(lit.value())
            }

            Lit::Str(ref lit) => {
                let value = lit.value();
                let value = value.trim();

                if let Ty::Enumeration(ref path) = *ty {
                    let variant = Ident::new(value, Span::call_site());
                    return Ok(DefaultValue::Enumeration(quote!(#path::#variant)));
                }

                // Parse special floating point values.
                if *ty == Ty::Float {
                    match value {
                        "inf" => {
                            return Ok(DefaultValue::Path(parse_str::<Path>(
                                "::core::f32::INFINITY",
                            )?));
                        }
                        "-inf" => {
                            return Ok(DefaultValue::Path(parse_str::<Path>(
                                "::core::f32::NEG_INFINITY",
                            )?));
                        }
                        "nan" => {
                            return Ok(DefaultValue::Path(parse_str::<Path>("::core::f32::NAN")?));
                        }
                        _ => (),
                    }
                }
                if *ty == Ty::Double {
                    match value {
                        "inf" => {
                            return Ok(DefaultValue::Path(parse_str::<Path>(
                                "::core::f64::INFINITY",
                            )?));
                        }
                        "-inf" => {
                            return Ok(DefaultValue::Path(parse_str::<Path>(
                                "::core::f64::NEG_INFINITY",
                            )?));
                        }
                        "nan" => {
                            return Ok(DefaultValue::Path(parse_str::<Path>("::core::f64::NAN")?));
                        }
                        _ => (),
                    }
                }

                // Rust doesn't have a negative literals, so they have to be parsed specially.
                if let Some(Ok(lit)) = value.strip_prefix('-').map(syn::parse_str::<Lit>) {
                    match lit {
                        Lit::Int(ref lit) if is_i32 && empty_or_is("i32", lit.suffix()) => {
                            // Initially parse into an i64, so that i32::MIN does not overflow.
                            let value: i64 = -lit.base10_parse()?;
                            return Ok(i32::try_from(value).map(DefaultValue::I32)?);
                        }
                        Lit::Int(ref lit) if is_i64 && empty_or_is("i64", lit.suffix()) => {
                            // Initially parse into an i128, so that i64::MIN does not overflow.
                            let value: i128 = -lit.base10_parse()?;
                            return Ok(i64::try_from(value).map(DefaultValue::I64)?);
                        }
                        Lit::Float(ref lit)
                            if *ty == Ty::Float && empty_or_is("f32", lit.suffix()) =>
                        {
                            return Ok(DefaultValue::F32(-lit.base10_parse()?));
                        }
                        Lit::Float(ref lit)
                            if *ty == Ty::Double && empty_or_is("f64", lit.suffix()) =>
                        {
                            return Ok(DefaultValue::F64(-lit.base10_parse()?));
                        }
                        Lit::Int(ref lit) if *ty == Ty::Float && lit.suffix().is_empty() => {
                            return Ok(DefaultValue::F32(-lit.base10_parse()?));
                        }
                        Lit::Int(ref lit) if *ty == Ty::Double && lit.suffix().is_empty() => {
                            return Ok(DefaultValue::F64(-lit.base10_parse()?));
                        }
                        _ => (),
                    }
                }
                match syn::parse_str::<Lit>(value) {
                    Ok(Lit::Str(_)) => (),
                    Ok(lit) => return DefaultValue::from_lit(ty, lit),
                    _ => (),
                }
                bail!("invalid default value: {}", quote!(#value));
            }
            _ => bail!("invalid default value: {}", quote!(#lit)),
        };

        Ok(default)
    }

    pub fn new(ty: &Ty) -> DefaultValue {
        match *ty {
            Ty::Float => DefaultValue::F32(0.0),
            Ty::Double => DefaultValue::F64(0.0),
            Ty::Int32 | Ty::Sint32 | Ty::Sfixed32 => DefaultValue::I32(0),
            Ty::Int64 | Ty::Sint64 | Ty::Sfixed64 => DefaultValue::I64(0),
            Ty::Uint32 | Ty::Fixed32 => DefaultValue::U32(0),
            Ty::Uint64 | Ty::Fixed64 => DefaultValue::U64(0),

            Ty::Bool => DefaultValue::Bool(false),
            Ty::String => DefaultValue::String(String::new()),
            Ty::Bytes(..) => DefaultValue::Bytes(Vec::new()),
            Ty::Enumeration(ref path) => DefaultValue::Enumeration(quote!(#path::default())),
        }
    }

    #[allow(unused)]
    pub fn owned(&self) -> TokenStream {
        match *self {
            DefaultValue::String(ref value) if value.is_empty() => {
                quote!(::prost::alloc::string::String::new())
            }
            DefaultValue::String(ref value) => quote!(#value.into()),
            DefaultValue::Bytes(ref value) if value.is_empty() => {
                quote!(::core::default::Default::default())
            }
            DefaultValue::Bytes(ref value) => {
                let lit = LitByteStr::new(value, Span::call_site());
                quote!(#lit.as_ref().into())
            }

            ref other => other.typed(),
        }
    }

    #[allow(unused)]
    pub fn typed(&self) -> TokenStream {
        if let DefaultValue::Enumeration(_) = *self {
            quote!(#self as i32)
        } else {
            quote!(#self)
        }
    }
}

impl ToTokens for DefaultValue {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        match *self {
            DefaultValue::F64(value) => value.to_tokens(tokens),
            DefaultValue::F32(value) => value.to_tokens(tokens),
            DefaultValue::I32(value) => value.to_tokens(tokens),
            DefaultValue::I64(value) => value.to_tokens(tokens),
            DefaultValue::U32(value) => value.to_tokens(tokens),
            DefaultValue::U64(value) => value.to_tokens(tokens),
            DefaultValue::Bool(value) => value.to_tokens(tokens),
            DefaultValue::String(ref value) => value.to_tokens(tokens),
            DefaultValue::Bytes(ref value) => {
                let byte_str = LitByteStr::new(value, Span::call_site());
                tokens.append_all(quote!(#byte_str as &[u8]));
            }
            DefaultValue::Enumeration(ref value) => value.to_tokens(tokens),
            DefaultValue::Path(ref value) => value.to_tokens(tokens),
        }
    }
}
