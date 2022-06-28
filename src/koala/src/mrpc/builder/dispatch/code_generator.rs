use std::collections::HashMap;
use std::path::PathBuf;

use proc_macro2::TokenStream;
use quote::quote;
use syn::Result;

use crate::mrpc::builder::{MethodIdentifier, RpcMethodInfo};

pub fn generate_marshal(method_id: &MethodIdentifier, ty: &str) -> Result<TokenStream> {
    let func_id = method_id.1;
    let rust_ty = syn::parse_str::<syn::Path>(&format!("codegen::{}", ty))?;
    let marshal = quote! {
        #func_id => {
            let ptr_backend = addr_backend as *mut #rust_ty;
            let msg_ref = unsafe { &*ptr_backend };
            msg_ref.marshal()
        },
    };
    Ok(marshal)
}

pub fn generate_unmarshal(method_id: &MethodIdentifier, ty: &str) -> Result<TokenStream> {
    let func_id = method_id.1;
    let rust_ty = syn::parse_str::<syn::Path>(&format!("codegen::{}", ty))?;
    let unmarshal = quote! {
        # func_id => {
            let msg = #rust_ty::unmarshal(ctx)?;
            let (ptr_app, ptr_backend) = msg.to_raw_parts();
            (ptr_app.addr().get(), ptr_backend.addr().get())
        },
    };
    Ok(unmarshal)
}

pub fn generate(
    include_file: PathBuf,
    method_type_mapping: &HashMap<MethodIdentifier, RpcMethodInfo>,
) -> Result<TokenStream> {
    let include_file = include_file.to_str().unwrap();

    let requests_marshal = method_type_mapping
        .iter()
        .map(|(id, info)| generate_marshal(id, &info.input_type))
        .collect::<Result<Vec<_>>>()?;

    let requests_unmarshal = method_type_mapping
        .iter()
        .map(|(id, info)| generate_unmarshal(id, &info.input_type))
        .collect::<Result<Vec<_>>>()?;

    let responses_marshal = method_type_mapping
        .iter()
        .map(|(id, info)| generate_marshal(id, &info.output_type))
        .collect::<Result<Vec<_>>>()?;

    let response_unmarshal = method_type_mapping
        .iter()
        .map(|(id, info)| generate_unmarshal(id, &info.output_type))
        .collect::<Result<Vec<_>>>()?;

    let dispatch = quote! {
        #![feature(strict_provenance)]

        use std::collections::BTreeMap;

        use interface::rpc::{MessageMeta, RpcMsgType};
        use mrpc_marshal::{SgList, ExcavateContext, ShmRecvMr, RpcMessage};
        use mrpc_marshal::{MarshalError, UnmarshalError};

        mod codegen {
            include!(#include_file);
        }

        #[no_mangle]
        pub extern "Rust" fn marshal(
            meta: &MessageMeta,
            addr_backend: usize,
        ) -> Result<SgList, MarshalError> {
            match meta.msg_type {
                RpcMsgType::Request => {
                    match meta.func_id {
                        #(#requests_marshal)*
                        _ => panic!("unknown func_id: {}, meta: {:?}", meta.func_id, meta),
                    }
                },
                RpcMsgType::Response => {
                    match meta.func_id {
                        #(#responses_marshal)*
                        _ => panic!("unknown func_id: {}, meta: {:?}", meta.func_id, meta),
                    }
                }
            }
        }

        #[no_mangle]
        pub unsafe extern "Rust" fn unmarshal(
            meta: &MessageMeta,
            ctx: &mut ExcavateContext<spin::Mutex<BTreeMap<usize, ShmRecvMr>>>
        ) -> Result<(usize, usize), UnmarshalError> {
            let addr_shm = match meta.msg_type {
                RpcMsgType::Request => {
                    match meta.func_id {
                        #(#requests_unmarshal)*
                        _ => panic!("unknown func_id: {}, meta: {:?}", meta.func_id, meta),
                    }
                },
                RpcMsgType::Response => {
                    match meta.func_id {
                        #(#response_unmarshal)*
                        _ => panic!("unknown func_id: {}, meta: {:?}", meta.func_id, meta),
                    }
                }
            };

            Ok(addr_shm)
        }
    };

    Ok(dispatch)
}
