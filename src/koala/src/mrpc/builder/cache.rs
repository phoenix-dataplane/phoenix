use std::path::Path;

pub fn check_cache<P: AsRef<Path>>(
    protos: &Vec<String>,
    cache_dir: P,
    proto_dir: P,
) -> (String, bool) {
    let mut checksum_ctx = md5::Context::new();
    for proto in protos.iter() {
        checksum_ctx.consume(proto.as_bytes());
    }
    let app_identifier = format!("{:0x}", checksum_ctx.compute());
    let cached_proto_dir = cache_dir.as_ref().join(&app_identifier).join(proto_dir.as_ref());
    if !cached_proto_dir.is_dir() {
        return (app_identifier, false);
    }
    for (idx, proto) in protos.iter().enumerate() {
        let filename = cached_proto_dir.join(format!{"{}.proto", idx});
        let proto_from_file = match std::fs::read_to_string(filename) {
            Ok(proto) => proto,
            Err(_) => return (app_identifier, false)
        };
        // check if proto file matches
        if !proto.eq(&proto_from_file) {
            return (app_identifier, false);
        }
    }
    todo!()
}

pub fn write_protos_to_cache<P: AsRef<Path>>(
    identifier: String,
    protos: &Vec<String>,
    cache_dir: P,
    proto_dir: P,
) {
    let app_folder = cache_dir.as_ref().join(identifier);
    if !app_folder.is_dir() {
        std::fs::create_dir(&app_folder).unwrap();
    }
    let proto_dir = app_folder.join(proto_dir.as_ref());


    for (idx, proto) in protos.iter().enumerate() {
        let filename = proto_dir.join(format!("{}.proto", idx));
        let file = std::fs::File::create(path)
    }
}