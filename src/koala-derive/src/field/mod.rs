mod message;
mod scalar;

use std::fmt;
use std::slice;

use anyhow::{bail, Error};
use proc_macro2::TokenStream;
use syn::{Attribute, Lit, LitBool, Meta, MetaList, MetaNameValue, NestedMeta};

#[derive(Clone)]
pub enum Field {
    Scalar(scalar::Field),
    Message(message::Field),
}

// dispatcher
impl Field {
    /// Creates a new `Field` from an iterator of field attributes.
    ///
    /// If the meta items are invalid, an error will be returned.
    /// If the field should be ignored, `None` is returned.
    pub fn new(attrs: Vec<Attribute>, inferred_tag: Option<u32>) -> Result<Option<Field>, Error> {
        let attrs = prost_attrs(attrs);
        let field = if let Some(field) = scalar::Field::new(&attrs, inferred_tag)? {
            Field::Scalar(field)
        } else if let Some(field) = message::Field::new(&attrs, inferred_tag)? {
            Field::Message(field)
        } else {
            bail!("no type attribute");
        };

        Ok(Some(field))
    }

    pub fn tags(&self) -> Vec<u32> {
        match *self {
            Field::Scalar(ref scalar) => vec![scalar.tag],
            Field::Message(ref message) => vec![message.tag],
        }
    }

    /// Returns a statement that writes additional SgEs to the SgList that the field has
    /// beyond the field's representation itself
    /// e.g., buffer in Vec
    /// ident should be `self.field`
    pub fn emplace(&self, ident: TokenStream) -> TokenStream {
        match *self {
            Field::Scalar(ref scalar) => scalar.emplace(ident),
            Field::Message(ref message) => message.emplace(ident),
        }
    }

    /// Returns a statement to recover additional information of the field from SgEs
    pub fn excavate(&self, ident: TokenStream) -> TokenStream {
        match *self {
            Field::Scalar(ref scalar) => scalar.excavate(ident),
            Field::Message(ref message) => message.excavate(ident),
        }
    }
    /// Returns an expression that evaluates how many SgEs that this field requires
    /// beyond the field representation itself
    /// ident should be `self.field`
    pub fn extent(&self, ident: TokenStream) -> TokenStream {
        match *self {
            Field::Scalar(ref scalar) => scalar.extent(ident),
            Field::Message(ref message) => message.extent(ident),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Label {
    /// An optional field.
    Optional,
    /// A required field.
    Required,
    /// A repeated field.
    Repeated,
}

impl Label {
    fn as_str(self) -> &'static str {
        match self {
            Label::Optional => "optional",
            Label::Required => "required",
            Label::Repeated => "repeated",
        }
    }

    fn variants() -> slice::Iter<'static, Label> {
        const VARIANTS: &[Label] = &[Label::Optional, Label::Required, Label::Repeated];
        VARIANTS.iter()
    }

    /// Parses a string into a field label.
    /// If the string doesn't match a field label, `None` is returned.
    fn from_attr(attr: &Meta) -> Option<Label> {
        if let Meta::Path(ref path) = *attr {
            for &label in Label::variants() {
                if path.is_ident(label.as_str()) {
                    return Some(label);
                }
            }
        }
        None
    }
}

impl fmt::Debug for Label {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl fmt::Display for Label {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Get the items belonging to the 'prost' list attribute, e.g. `#[prost(foo, bar="baz")]`.
fn prost_attrs(attrs: Vec<Attribute>) -> Vec<Meta> {
    attrs
        .iter()
        .flat_map(Attribute::parse_meta)
        .flat_map(|meta| match meta {
            // prost's attrs are in lists
            Meta::List(MetaList { path, nested, .. }) => {
                if path.is_ident("prost") {
                    // extract all attrs in the MetaList
                    nested.into_iter().collect()
                } else {
                    Vec::new()
                }
            }
            _ => Vec::new(),
        })
        .flat_map(|attr| -> Result<_, _> {
            match attr {
                // the attrs in the #[prost(...)] list cannot be literal.
                NestedMeta::Meta(attr) => Ok(attr),
                NestedMeta::Lit(lit) => bail!("invalid prost attribute: {:?}", lit),
            }
        })
        .collect()
}

pub fn set_option<T>(option: &mut Option<T>, value: T, message: &str) -> Result<(), Error>
where
    T: fmt::Debug,
{
    if let Some(ref existing) = *option {
        bail!("{}: {:?} and {:?}", message, existing, value);
    }
    *option = Some(value);
    Ok(())
}

pub fn set_bool(b: &mut bool, message: &str) -> Result<(), Error> {
    if *b {
        bail!("{}", message);
    } else {
        *b = true;
        Ok(())
    }
}

fn bool_attr(key: &str, attr: &Meta) -> Result<Option<bool>, Error> {
    if !attr.path().is_ident(key) {
        return Ok(None);
    }
    match *attr {
        Meta::Path(..) => Ok(Some(true)),
        Meta::List(ref meta_list) => {
            // TODO(rustlang/rust#23121): slice pattern matching would make this much nicer.
            if meta_list.nested.len() == 1 {
                if let NestedMeta::Lit(Lit::Bool(LitBool { value, .. })) = meta_list.nested[0] {
                    return Ok(Some(value));
                }
            }
            bail!("invalid {} attribute", key);
        }
        Meta::NameValue(MetaNameValue {
            lit: Lit::Str(ref lit),
            ..
        }) => lit.value().parse::<bool>().map_err(Error::from).map(Some),
        Meta::NameValue(MetaNameValue {
            lit: Lit::Bool(LitBool { value, .. }),
            ..
        }) => Ok(Some(value)),
        _ => bail!("invalid {} attribute", key),
    }
}

/// Checks if an attribute matches a word.
fn word_attr(key: &str, attr: &Meta) -> bool {
    if let Meta::Path(ref path) = *attr {
        path.is_ident(key)
    } else {
        false
    }
}

pub(super) fn tag_attr(attr: &Meta) -> Result<Option<u32>, Error> {
    // parse field tags
    if !attr.path().is_ident("tag") {
        return Ok(None);
    }
    match *attr {
        Meta::List(ref meta_list) => {
            // TODO(rustlang/rust#23121): slice pattern matching would make this much nicer.
            if meta_list.nested.len() == 1 {
                if let NestedMeta::Lit(Lit::Int(ref lit)) = meta_list.nested[0] {
                    return Ok(Some(lit.base10_parse()?));
                }
            }
            bail!("invalid tag attribute: {:?}", attr);
        }
        Meta::NameValue(ref meta_name_value) => match meta_name_value.lit {
            Lit::Str(ref lit) => lit.value().parse::<u32>().map_err(Error::from).map(Some),
            Lit::Int(ref lit) => Ok(Some(lit.base10_parse()?)),
            _ => bail!("invalid tag attribute: {:?}", attr),
        },
        _ => bail!("invalid tag attribute: {:?}", attr),
    }
}
