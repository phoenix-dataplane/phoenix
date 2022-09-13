const PROTO_DIR: &str = "../proto/hotel_microservices";

const PROTOS: &[&str] = &[
    "../proto/hotel_microservices/geo.proto",
    "../proto/hotel_microservices/rate.proto",
    "../proto/hotel_microservices/search.proto",
    "../proto/hotel_microservices/profile.proto",
];

fn main() -> Result<(), Box<dyn std::error::Error>> {
    for proto in PROTOS.iter() {
        println!("cargo:rerun-if-changed={proto}");
    }
    let builder = mrpc_build::configure();
    builder.compile(PROTOS, &[PROTO_DIR])?;
    Ok(())
}
