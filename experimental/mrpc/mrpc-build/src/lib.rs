//! The code of `mrpc-build` larged bases on `tonic-build`.
//!
//! `mrpc-build` compiles `proto` files via `prost` and generates service stubs
//! and proto definitiones for use with `mrpc`.
//!
//! # Required dependencies
//!
//! ```toml
//! [dependencies]
//! # TODO(cjr): move mrpc to a separate crate, low priority
//! libphoenix = { path = <path-to-libphoenix>, version = <libphoenix-version> }
//! prost = { patht = <path-to-custom-prost>, features = ["mrpc"] }
//!
//! [build-dependencies]
//! mrpc-build = { path = <path-to-mrpc-build> }
//! ```
//!
//! # Examples
//! Simple
//!
//! ```rust,no_run
//! fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     mrpc_build::compile_protos("proto/service.proto")?;
//!     Ok(())
//! }
//! ```
//!
//! Configuration
//!
//! ```rust,no_run
//! fn main() -> Result<(), Box<dyn std::error::Error>> {
//!    mrpc_build::configure()
//!         .build_server(false)
//!         .compile(
//!             &["proto/helloworld/helloworld.proto"],
//!             &["proto/helloworld"],
//!         )?;
//!    Ok(())
//! }
//!```

#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]
#![warn(missing_debug_implementations)]

use std::collections::BTreeSet;

use proc_macro2::TokenStream;

mod prost;
pub use prost::{compile_protos, configure, Builder};

/// Service code generation for client
pub mod client;
/// Service code generation for Server
pub mod server;

/// Attributes that will be added to `mod` and `struct` items.
pub mod attribute;

/// Service generation trait.
///
/// This trait can be implemented and consumed
/// by `client::generate` and `server::generate`
/// to allow any codegen module to generate service
/// abstractions.
pub trait Service {
    /// Comment type.
    type Comment: AsRef<str>;

    /// Method type.
    type Method: Method;

    /// Name of service.
    fn name(&self) -> &str;
    /// Package name of service.
    fn package(&self) -> &str;
    /// Identifier used to generate type name.
    fn identifier(&self) -> &str;
    /// Methods provided by service.
    fn methods(&self) -> &[Self::Method];
    /// Get comments about this item.
    fn comment(&self) -> &[Self::Comment];
}

/// Method generation trait.
///
/// Each service contains a set of generic
/// `Methods`'s that will be used by codegen
/// to generate abstraction implementations for
/// the provided methods.
pub trait Method {
    /// Comment type.
    type Comment: AsRef<str>;

    /// Name of method.
    fn name(&self) -> &str;
    /// Identifier used to generate type name.
    fn identifier(&self) -> &str;
    /// Get comments about this item.
    fn comment(&self) -> &[Self::Comment];
    /// Type name of request and response.
    fn request_response_name(
        &self,
        proto_path: &str,
        compile_well_known_types: bool,
    ) -> (TokenStream, TokenStream);
    /// Proto pacakge name of request and response
    fn request_response_package(&self, proto_path: &str) -> (Option<String>, Option<String>);
}

// Returns a full path of a service compatible to gRPC.
fn get_service_path<S: Service>(package: &str, service: &S) -> String {
    format!(
        "{}{}{}",
        package,
        if package.is_empty() { "" } else { "." },
        service.identifier(),
    )
}

// Returns a full path of a method compatible to gRPC.
fn get_method_path<S: Service<Method = M>, M: Method>(
    package: &str,
    service: &S,
    method: &M,
) -> String {
    format!(
        "/{}{}{}/{}",
        package,
        if package.is_empty() { "" } else { "." },
        service.identifier(),
        method.identifier(),
    )
}

// Calculate SERVICE_ID for mRPC NamedService.
fn mrpc_get_service_id(path: &str) -> u32 {
    crc32fast::hash(path.as_bytes())
}

// Calculate FUNC_ID for mRPC remote procedures.
fn mrpc_get_func_id(path: &str) -> u32 {
    crc32fast::hash(path.as_bytes())
}

// Generate a singular line of a doc comment
fn generate_doc_comment<S: AsRef<str>>(comment: S) -> TokenStream {
    use proc_macro2::{Delimiter, Group, Ident, Literal, Punct, Spacing, Span};
    use quote::TokenStreamExt;

    let mut doc_stream = TokenStream::new();

    doc_stream.append(Ident::new("doc", Span::call_site()));
    doc_stream.append(Punct::new('=', Spacing::Alone));
    doc_stream.append(Literal::string(comment.as_ref()));

    let group = Group::new(Delimiter::Bracket, doc_stream);

    let mut stream = TokenStream::new();
    stream.append(Punct::new('#', Spacing::Alone));
    stream.append(group);
    stream
}

// Generate a larger doc comment composed of many lines of doc comments
fn generate_doc_comments<T: AsRef<str>>(comments: &[T]) -> TokenStream {
    let mut stream = TokenStream::new();

    for comment in comments {
        stream.extend(generate_doc_comment(comment));
    }

    stream
}

fn naive_snake_case(name: &str) -> String {
    let mut s = String::new();
    let mut it = name.chars().peekable();

    while let Some(x) = it.next() {
        s.push(x.to_ascii_lowercase());
        if let Some(y) = it.peek() {
            if y.is_uppercase() {
                s.push('_');
            }
        }
    }

    s
}

fn get_proto_packages<T: Service>(service: &T, proto_path: &str) -> BTreeSet<String> {
    let mut proto_packages = BTreeSet::new();
    for method in service.methods() {
        let (input_package, output_package) = method.request_response_package(proto_path);
        if let Some(pkg) = input_package {
            let pkg_srcs = format!("{}::proto::PROTO_SRCS", pkg);
            proto_packages.insert(pkg_srcs);
        }
        if let Some(pkg) = output_package {
            let pkg_srcs = format!("{}::proto::PROTO_SRCS", pkg);
            proto_packages.insert(pkg_srcs);
        }
    }
    proto_packages
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_snake_case() {
        for case in &[
            ("Service", "service"),
            ("ThatHasALongName", "that_has_a_long_name"),
            ("greeter", "greeter"),
            ("ABCServiceX", "a_b_c_service_x"),
        ] {
            assert_eq!(naive_snake_case(case.0), case.1)
        }
    }
}
