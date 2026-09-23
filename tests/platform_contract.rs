use bornengine_cli::platform::{
    BuildTarget, HostPlatform, PerryCapabilities, TargetRequest, resolve_target,
};

fn capabilities() -> PerryCapabilities {
    PerryCapabilities::from_compile_help(
        "--target <TARGET>\nTarget platform: ios-simulator, ios, visionos, android, web, wasm, windows, linux (default: native host)",
    )
}

#[test]
fn parses_targets_advertised_by_perry() {
    let selected = resolve_target(
        &TargetRequest {
            os: None,
            target: Some("ios-simulator".to_owned()),
        },
        &capabilities(),
        HostPlatform::Linux,
    )
    .unwrap();

    assert_eq!(selected.target, BuildTarget::IosSimulator);
    assert_eq!(selected.perry_target.as_deref(), Some("ios-simulator"));
}

#[test]
fn defaults_to_host_and_maps_friendly_linux_option() {
    let caps = capabilities();
    let implicit = resolve_target(&TargetRequest::default(), &caps, HostPlatform::Linux).unwrap();
    let explicit = resolve_target(
        &TargetRequest {
            os: Some("linux".to_owned()),
            target: None,
        },
        &caps,
        HostPlatform::Linux,
    )
    .unwrap();

    assert_eq!(implicit.target, BuildTarget::Linux);
    assert_eq!(implicit.perry_target, None);
    assert_eq!(explicit.target, BuildTarget::Linux);
    assert_eq!(explicit.perry_target.as_deref(), Some("linux"));
}

#[test]
fn implicit_host_build_does_not_require_a_named_cross_target() {
    let implicit = resolve_target(
        &TargetRequest::default(),
        &PerryCapabilities::default(),
        HostPlatform::Linux,
    )
    .unwrap();

    assert_eq!(implicit.target, BuildTarget::Linux);
    assert_eq!(implicit.perry_target, None);
}

#[test]
fn macos_uses_native_perry_target() {
    let selected = resolve_target(
        &TargetRequest {
            os: Some("macos".to_owned()),
            target: None,
        },
        &capabilities(),
        HostPlatform::MacOS,
    )
    .unwrap();

    assert_eq!(selected.target, BuildTarget::MacOS);
    assert_eq!(selected.perry_target, None);
}

#[test]
fn rejects_targets_not_advertised_by_installed_perry() {
    let result = resolve_target(
        &TargetRequest {
            os: Some("tvos".to_owned()),
            target: None,
        },
        &capabilities(),
        HostPlatform::Linux,
    );

    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("does not advertise")
    );
}

#[test]
fn accepts_new_exact_targets_only_when_advertised_by_perry() {
    let capabilities = PerryCapabilities::from_compile_help(
        "Target platform: android, android-arm64, linux (default: native)",
    );

    let exact = resolve_target(
        &TargetRequest {
            os: None,
            target: Some("android-arm64".to_owned()),
        },
        &capabilities,
        HostPlatform::Linux,
    )
    .unwrap();

    assert_eq!(exact.target, BuildTarget::Android);
    assert_eq!(exact.perry_target.as_deref(), Some("android-arm64"));
}

#[test]
fn rejects_cross_os_run_requests() {
    let selected = resolve_target(
        &TargetRequest {
            os: Some("windows".to_owned()),
            target: None,
        },
        &capabilities(),
        HostPlatform::Linux,
    )
    .unwrap();

    assert!(!selected.can_run_on(HostPlatform::Linux));
}

#[test]
fn output_extensions_match_perry_output_formats() {
    assert_eq!(BuildTarget::Windows.output_extension(), Some("exe"));
    assert_eq!(BuildTarget::Web.output_extension(), Some("html"));
    assert_eq!(BuildTarget::Linux.output_extension(), None);
}

#[test]
fn parses_host_operating_system_names() {
    assert_eq!(HostPlatform::parse("linux"), HostPlatform::Linux);
    assert_eq!(HostPlatform::parse("windows"), HostPlatform::Windows);
    assert_eq!(HostPlatform::parse("darwin"), HostPlatform::MacOS);
    assert_eq!(
        HostPlatform::current(),
        HostPlatform::parse(std::env::consts::OS)
    );
}
