pub const BORNENGINE_PACKAGE: &str = "@bornengine/engine";
pub const LEGACY_ENGINE_PACKAGE: &str = "@bloomengine/engine";
pub const LEGACY_JOLT_PACKAGE: &str = "@bloomengine/jolt-prebuilt";
pub const ENGINE_PACKAGES: [&str; 2] = [BORNENGINE_PACKAGE, LEGACY_ENGINE_PACKAGE];

pub fn is_engine_package(name: &str) -> bool {
    name == BORNENGINE_PACKAGE || name == LEGACY_ENGINE_PACKAGE
}

pub fn engine_subpath(name: &str, subpath: &str) -> String {
    format!("{name}/{subpath}")
}

pub fn native_library_allow_pattern(name: &str) -> String {
    format!("{name}/*")
}
