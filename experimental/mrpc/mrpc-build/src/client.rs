use proc_macro2::TokenStream;

use crate::attribute::Attributes;
use crate::{
    generate_doc_comments, get_method_path, get_proto_packages, get_service_path, mrpc_get_func_id,
    mrpc_get_service_id, naive_snake_case, Method, Service,
};

/// Generate service for client.
///
/// This takes some `Service` and will generate a `TokenStream` that contains
/// a public module with the generated client.
pub fn generate<T: Service>(
    service: &T,
    emit_package: bool,
    proto_path: &str,
    compile_well_known_types: bool,
    attributes: &Attributes,
) -> TokenStream {
    let service_ident = quote::format_ident!("{}Client", service.name());
    let client_mod = quote::format_ident!("{}_client", naive_snake_case(service.name()));
    let methods = generate_methods(service, emit_package, proto_path, compile_well_known_types);

    let service_doc = generate_doc_comments(service.comment());

    let package = if emit_package { service.package() } else { "" };
    let path = get_service_path(package, service);
    let service_id = mrpc_get_service_id(&path);

    let mod_attributes = attributes.for_mod(package);
    let struct_attributes = attributes.for_struct(&path);

    let proto_packages = get_proto_packages(service, proto_path);

    let proto_srcs = proto_packages
        .into_iter()
        .map(|x| syn::parse_str::<syn::Path>(&x).unwrap());

    quote::quote! {
        /// Generate client implementations.
        #(#mod_attributes)*
        pub mod #client_mod {
            use ::mrpc::stub::{ClientStub, NamedService};

            #service_doc
            #(#struct_attributes)*
            #[derive(Debug)]
            pub struct #service_ident {
                stub: ClientStub,
            }

            impl #service_ident {
                fn update_protos() -> Result<(), ::mrpc::Error> {
                    let srcs = [#(#proto_srcs),*].concat();
                    ::mrpc::stub::update_protos(srcs.as_slice())
                }

                pub fn connect<A: std::net::ToSocketAddrs>(dst: A) -> Result<Self, ::mrpc::Error> {
                    // use the cmid builder to create a CmId.
                    // no you shouldn't rely on cmid here anymore. you should have your own rpc endpoint
                    // cmid communicates directly to the transport engine. you need to pass your raw rpc
                    // request/response to/from the rpc engine rather than the transport engine.
                    // let stub = phoenix_syscalls::mrpc::cm::MrpcStub::set_transport(phoenix_syscalls::mrpc::cm::TransportType::Rdma)?;
                    Self::update_protos()?;
                    let stub = ClientStub::connect(dst).unwrap();
                    Ok(Self {
                        stub,
                    })
                }
                pub fn multi_connect<A: std::net::ToSocketAddrs>(dsts: Vec<A>) -> Result<Self, ::mrpc::Error> {
                    // use the cmid builder to create a CmId.
                    // no you shouldn't rely on cmid here anymore. you should have your own rpc endpoint
                    // cmid communicates directly to the transport engine. you need to pass your raw rpc
                    // request/response to/from the rpc engine rather than the transport engine.
                    // let stub = phoenix_syscalls::mrpc::cm::MrpcStub::set_transport(phoenix_syscalls::mrpc::cm::TransportType::Rdma)?;
                    Self::update_protos()?;
                    let stub = ClientStub::multi_connect(dsts).unwrap();
                    Ok(Self {
                        stub,
                    })
                }
                #methods
            }

            impl NamedService for #service_ident {
                const SERVICE_ID: u32 = #service_id;
                const NAME: &'static str = #path;
            }
        }
    }
}

fn generate_methods<T: Service>(
    service: &T,
    emit_package: bool,
    proto_path: &str,
    compile_well_known_types: bool,
) -> TokenStream {
    let mut stream = TokenStream::new();
    let package = if emit_package { service.package() } else { "" };

    // println!("cargo:warning={}", "================generate_methods================");
    for method in service.methods() {
        let path = get_method_path(package, service, method);
        let func_id = mrpc_get_func_id(&path);
        let service_id = mrpc_get_service_id(&get_service_path(package, service));

        stream.extend(generate_doc_comments(method.comment()));

        // mRPC current doesn't not support streaming
        // Generate unary
        let ident = quote::format_ident!("{}", method.name());

        let (request, response) =
            method.request_response_name(proto_path, compile_well_known_types);

        let method = quote::quote! {
            pub fn #ident(
                &self,
                req: impl ::mrpc::IntoWRef<#request>
            ) -> impl std::future::Future<
                Output = Result<::mrpc::RRef<#response>, ::mrpc::Status>
            > + '_ {
                let call_id = self.stub.initiate_call();

                self.stub.unary(#service_id, #func_id, call_id, req.into_wref())
            }
        };

        stream.extend(method);
    }

    stream
}
