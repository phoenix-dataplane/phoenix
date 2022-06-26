use std::path::PathBuf;
use std::collections::HashMap;

use proc_macro2::TokenStream;
use quote::quote;
use crate::mrpc::builder::{MethodIdentifier, RpcMethodInfo};

pub fn generate_marshal(method_id: MethodIdentifier, mut ty: String) -> TokenStream {
    let func_id = method_id.1;
    ty = format!("codegen::{}", ty);
    let input_type = syn::parse_str::<syn::Path>(&ty).unwrap();
    quote! {
        #func_id => {
            let ptr_backend = msg.addr_backend as *mut #input_type
            let msg_ref = unsafe { &*ptr_backend };
            msg_ref.marshal().unwrap()
        },
    }
}



pub fn generate(
    include_file: PathBuf, 
    method_type_mapping: &HashMap<MethodIdentifier, RpcMethodInfo>
) -> TokenStream {
    let include_file = include_file.to_str().unwrap();

    quote! {
        use ipc::ptr::ShmPtr;
        use interface::ipc::MessageMeta;
        use mrpc-marshal::mrpc::marshal::{SgList, ExcavateContext};
        use mrpc-marshal::marshal::{MarshalError, UnmarshalError};

        mod codegen {
            include!(#include_file);
        }

        pub extern "Rust" fn marshal(messate_meta: &MessageMeta) -> Result<SgList, MarshalError> {
            todo!()
        }

        pub unsafe extern "Rust" fn unmarshal(ctx: &mut ExcavateContext) -> Result<ShmPtr<Self>, UnmarshalError> {

        }
    }
}