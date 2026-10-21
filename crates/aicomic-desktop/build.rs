fn main() {
    println!("cargo:rerun-if-changed=../../packaging/icons/icon.ico");
    println!("cargo:rerun-if-changed=../../packaging/windows/app.manifest");
    #[cfg(windows)]
    {
        let mut resource = winresource::WindowsResource::new();
        resource
            .set_icon("../../packaging/icons/icon.ico")
            .set_manifest_file("../../packaging/windows/app.manifest")
            .set("ProductName", "星晴")
            .set("FileDescription", "星晴 · AI 漫剧创作")
            .set("InternalName", "aicomic-tool")
            .set("OriginalFilename", "aicomic-tool.exe");
        resource
            .compile()
            .expect("Windows icon and manifest compilation failed");
    }
}
