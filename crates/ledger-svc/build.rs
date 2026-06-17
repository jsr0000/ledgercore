// Compile proto/ledger.proto to Rust at build time. Output lands in
// OUT_DIR and is pulled in by lib.rs via tonic::include_proto!.

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let proto_root = "../../proto";
    let proto_file = "../../proto/ledger.proto";

    tonic_build::configure()
        .build_server(true)
        .build_client(true) // generated client is useful for integration tests
        .compile_protos(&[proto_file], &[proto_root])?;

    println!("cargo:rerun-if-changed={proto_file}");
    Ok(())
}
