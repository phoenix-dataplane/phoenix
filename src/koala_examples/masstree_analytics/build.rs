use cmake;

fn main() {
    // Builds the project in the directory located in `libmt_index`, installing it
    // into $OUT_DIR
    let dst = cmake::build("libmt_index");

    println!("cargo:rustc-link-search=native={}", dst.display());
    println!("cargo:rustc-link-lib=static=mt_index");
}
