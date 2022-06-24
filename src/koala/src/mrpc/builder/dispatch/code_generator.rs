use std::path::PathBuf;
use std::collections::HashMap;

use proc_macro2::TokenStream;
use quote::quote;
use crate::mrpc::builder::{MethodIdentifier, RpcMethodInfo};

pub fn marshal_request(method_id: MethodIdentifier, mut input_type: String) -> TokenStream {
    let func_id = method_id.1;
    input_type = format!("codegen::{}", input_type);
    let input_type = syn::parse_str::<syn::Path>(&input_type).unwrap();
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
    method_type_mapping: HashMap<MethodIdentifier, RpcMethodInfo>
) -> TokenStream {
    quote! {
        use ipc::ptr::ShmPtr;
        use koala::mrpc::marshal::{MessageMeta, SgList, ExcavateContext};
        use koala::mrpc::marshal::{MarshalError, UnmarshalError};

        mod codegen {
            include!();
        }

        pub extern "Rust" fn marshal(messate_meta: &MessageMeta) -> Result<SgList, MarshalError> {

        }

        pub unsafe extern "Rust" fn unmarshal(ctx: &mut ExcavateContext) -> Result<ShmPtr<Self>, UnmarshalError> {

        }
    }
}