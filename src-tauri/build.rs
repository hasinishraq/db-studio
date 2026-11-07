fn main() {
    // Tauri embeds its Windows manifest in the app binary only, but the dialog plugin needs Common
    // Controls v6 to load at all, so test binaries would die with STATUS_ENTRYPOINT_NOT_FOUND.
    // Linking the manifest ourselves covers every binary of this package.
    #[cfg(windows)]
    {
        let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("windows-app-manifest.xml");
        println!("cargo:rerun-if-changed=windows-app-manifest.xml");
        println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
        println!("cargo:rustc-link-arg=/MANIFESTINPUT:{}", manifest.display());
        let windows = tauri_build::WindowsAttributes::new_without_app_manifest();
        let attributes = tauri_build::Attributes::new().windows_attributes(windows);
        if let Err(error) = tauri_build::try_build(attributes) {
            panic!("{error:#}");
        }
    }
    #[cfg(not(windows))]
    tauri_build::build()
}
