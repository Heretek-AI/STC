//! Regenerate `cockpit/src/api-types.ts` from the ts-rs DTOs.
//! Usage: `cargo run -p studio-web --bin studio-web-gen-ts`

fn main() {
    let out = studio_web::api::export_ts();
    let path =
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../cockpit/src/api-types.ts");
    std::fs::write(&path, out).expect("write api-types.ts");
    eprintln!("[gen-ts] wrote {}", path.display());
}
