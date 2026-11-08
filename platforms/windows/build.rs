fn main() {
    println!("cargo:rerun-if-changed=../../assets/icons/cloudboard.ico");
    println!("cargo:rerun-if-changed=app.manifest");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("../../assets/icons/cloudboard.ico")
            .set_manifest_file("app.manifest")
            .set("ProductName", "CloudBoard")
            .set("FileDescription", "CloudBoard 27 智慧课堂白板")
            .set("CompanyName", "CloudBoard")
            .set("LegalCopyright", "CloudBoard contributors")
            .set("OriginalFilename", "cloudboard.exe");
        if let Err(e) = res.compile() {
            // Cross-checking from non-Windows hosts has no resource compiler.
            println!("cargo:warning=resource compilation skipped: {e}");
        }
    }
}
