use std::path::Path;

pub fn check_cache<P: AsRef<Path>>(
    protos: &Vec<String>,
    // dir to backend build cache
    cache_dir: P,
    // directory name where the proto files are stored
    // should be a directory name/path
    // relative to `cache_dir`
    proto_dir: &str,
) -> std::io::Result<(String, bool)> {
    let mut checksum_ctx = md5::Context::new();
    for proto in protos.iter() {
        checksum_ctx.consume(proto.as_bytes());
    }
    let app_identifier = format!("{:0x}", checksum_ctx.compute());
    // protos are stored in cache_dir/proto_dir
    let cached_proto_dir = cache_dir.as_ref().join(&app_identifier).join(proto_dir);
    if !cached_proto_dir.is_dir() {
        return Ok((app_identifier, false));
    }
    for (idx, proto) in protos.iter().enumerate() {
        let filename = cached_proto_dir.join(format! {"{}.proto", idx});
        let proto_from_file = std::fs::read_to_string(filename)?;
        // check if proto file matches
        if !proto.eq(&proto_from_file) {
            return Ok((app_identifier, false));
        }
    }

    Ok((app_identifier, true))
}

pub fn write_protos_to_cache<P: AsRef<Path>>(
    identifier: &str,
    protos: &Vec<String>,
    // dir to backend build cache
    cache_dir: P,
    // directory name where the proto files are stored
    // should be a directory name/path
    // relative to `cache_dir`
    proto_dir: &str,
) -> std::io::Result<()> {
    let app_folder = cache_dir.as_ref().join(identifier);
    if !app_folder.is_dir() {
        std::fs::create_dir(&app_folder)?;
    }

    let proto_dir = app_folder.join(proto_dir);
    if !proto_dir.is_dir() {
        std::fs::create_dir(&proto_dir)?;
    }

    for (idx, proto) in protos.iter().enumerate() {
        let filename = proto_dir.join(format!("{}.proto", idx));
        std::fs::write(filename, proto)?;
    }

    Ok(())
}
