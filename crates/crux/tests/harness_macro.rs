#[crux::harness]
pub struct MyHarness {
    pub memory_mb: u64,
    pub cpu_millicores: u64,
    pub timeout_seconds: u64,
    pub network_access: bool,
}

#[test]
fn harness_generates_to_profile() {
    let h = MyHarness {
        memory_mb: 1024,
        cpu_millicores: 2000,
        timeout_seconds: 600,
        network_access: true,
    };
    let profile = h.to_profile("my-harness-v1");
    assert_eq!(profile.id, "my-harness-v1");
    assert_eq!(profile.resources.memory_mb, 1024);
    assert!(profile.network_access);
}

#[test]
fn harness_generates_default() {
    let h = MyHarness::default();
    assert_eq!(h.memory_mb, 512);
    assert_eq!(h.timeout_seconds, 300);
    assert!(!h.network_access);
}
