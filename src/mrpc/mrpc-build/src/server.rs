use proc_macro2::TokenStream;

use crate::attribute::Attributes;
use crate::{
    generate_doc_comments, get_method_path, get_proto_packages, get_service_path, mrpc_get_func_id,
    mrpc_get_service_id, naive_snake_case, Method, Service,
};

/// Generate service for server.
///
/// This takes some `Service` and will generate a `TokenStream` that contains
/// a public module with the generated server.
pub fn generate<T: Service>(
    service: &T,
    emit_package: bool,
    proto_path: &str,
    compile_well_known_types: bool,
    attributes: &Attributes,
) -> TokenStream {
    let methods = generate_methods(service, proto_path, compile_well_known_types);

    let server_service = quote::format_ident!("{}Server", service.name());
    let server_trait = quote::format_ident!("{}", service.name());
    let server_mod = quote::format_ident!("{}_server", naive_snake_case(service.name()));

    let generated_trait = generate_trait(
        service,
        proto_path,
        compile_well_known_types,
        server_trait.clone(),
    );

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
        /// Generated server implementations.
        #(#mod_attributes)*
        pub mod #server_mod {
            use ::mrpc::stub::{NamedService, RpcMessage, Service};

            #generated_trait

            #service_doc
            #(#struct_attributes)*
            #[derive(Debug)]
            // Translate erased message to concrete type, and call the inner callback function.
            // Translate the reply type to erased message again and put to write shared heap.
            pub struct #server_service<T: #server_trait> {
                inner: T,
            }

            impl<T: #server_trait> #server_service<T> {
                fn update_protos() -> Result<(), ::mrpc::Error> {
                    let srcs = [#(#proto_srcs),*].concat();
                    ::mrpc::stub::update_protos(srcs.as_slice())
                }

                pub fn new(inner: T) -> Self {
                    // TODO: handle error here
                    Self::update_protos().unwrap();;
                    Self { inner }
                }
            }

            impl<T: #server_trait> NamedService for #server_service<T> {
                const SERVICE_ID: u32 = #service_id;
                const NAME: &'static str = #path;
            }

            impl<T: #server_trait> Service for #server_service<T> {
                fn call(
                    &self,
                    req: ::mrpc::MessageErased,
                    reclaim_buffer: &::mrpc::stub::ReclaimBuffer,
                ) -> (::mrpc::MessageErased, u64) {
                    let conn_id = req.meta.conn_id;
                    let call_id = req.meta.call_id;
                    let func_id = req.meta.func_id;
                    // todo!()
                    match func_id {
                        #methods
                        _ => {
                            todo!("error handling for unknown func_id: {}", func_id);
                        }
                    }
                }
            }
        }
    }
}

fn generate_trait<T: Service>(
    service: &T,
    proto_path: &str,
    compile_well_known_types: bool,
    server_trait: syn::Ident,
) -> TokenStream {
    let trait_methods = generate_trait_methods(service, proto_path, compile_well_known_types);

    quote::quote! {
        pub trait #server_trait: Send + Sync + 'static {
            #trait_methods
        }
    }
}

fn generate_trait_methods<T: Service>(
    service: &T,
    proto_path: &str,
    compile_well_known_types: bool,
) -> TokenStream {
    let mut stream = TokenStream::new();

    for method in service.methods() {
        let name = quote::format_ident!("{}", method.name());

        let (req_type, res_type) =
            method.request_response_name(proto_path, compile_well_known_types);

        let method_doc = generate_doc_comments(method.comment());

        // mRPC does not support streaming
        let method = quote::quote! {
            #method_doc
            fn #name(
                &self,
                request: ::mrpc::shmview::ShmView<#req_type>
            ) -> Result<&RpcMessage<#res_type>, ::mrpc::Status>;
        };

        stream.extend(method);
    }

    stream
}

fn generate_methods<T: Service>(
    service: &T,
    proto_path: &str,
    compile_well_known_types: bool,
) -> TokenStream {
    let mut stream = TokenStream::new();
    let package = service.package();

    for method in service.methods() {
        let service_id = mrpc_get_service_id(&get_service_path(package, service));

        let func_id = mrpc_get_func_id(&get_method_path(package, service, method));
        let func_ident = quote::format_ident!("{}", method.name());

        let (_req_type, _res_type) =
            method.request_response_name(proto_path, compile_well_known_types);
        // TODO
        let match_branch = quote::quote! {
            #func_id => {
                // let req_view: mrpc::shmview::ShmView<'_, #req_type> = mrpc::stub::service_pre_handler(&req, &());
                let req_view = ::mrpc::stub::service_pre_handler(&req, reclaim_buffer);
                match self.inner.#func_ident(req_view) {
                    Ok(reply) => {
                        ::mrpc::stub::service_post_handler(reply, conn_id, #service_id, #func_id, call_id)
                    }
                    Err(_status) => {
                        todo!();
                    }
                }
            },
        };

        stream.extend(match_branch);
    }

    stream
}
