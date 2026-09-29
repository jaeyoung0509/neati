use neati_lib::cleaner::{LifecycleProviderRegistry, OwnerProviderRegistry};
use neati_lib::models::{
    CacheManagementMode, CacheSizeSemantics, Category, CleanStrategy, CleanerFamily, NeatiSettings,
    NeverCancelled, RiskTier,
};
use neati_lib::scanner::{DirectoryScanner, ScanEngine};
use neati_lib::signatures::{SignatureLoader, SignatureRegistry};
use neati_platform::paths::SimulatedPaths;
use neati_platform::{PathFlavor, PlatformEnvironment};
use std::fs;
use std::sync::Arc;

#[test]
fn temp_aliases_produce_one_cleanup_unit() {
    let fixture = tempfile::tempdir().unwrap();
    let temp = fixture.path().join("temp");
    let unit = temp.join("codex-fixture");
    fs::create_dir_all(&unit).unwrap();
    fs::write(unit.join("payload.bin"), vec![1u8; 4096]).unwrap();
    let environment = PlatformEnvironment::simulated(PathFlavor::current())
        .with_home(fixture.path())
        .with_temp_dir(&temp);
    let registry = SignatureRegistry::load_embedded_catalog().unwrap();
    let mut signature = registry.get("system.developer_temp").unwrap().clone();
    signature.min_age_days = Some(0);
    let items = DirectoryScanner::scan_signature(
        &signature,
        &environment,
        &NeverCancelled,
        &neati_lib::applications::RunningApplications::from_process_names(["fixture-idle".into()])
            .with_open_file_probe(std::sync::Arc::new(
                neati_platform::open_files::FixedOpenFileProbe(
                    neati_platform::open_files::OpenFileState::Idle,
                ),
            )),
    );
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].size.logical, 4096);
    assert_eq!(items[0].path, unit.to_string_lossy());
}

#[test]
fn clang_cache_has_an_exact_root_and_compiler_guards_without_an_age_gate() {
    let fixture = tempfile::tempdir().unwrap();
    let cache_root = fixture.path().join("C");
    let modules = cache_root.join("clang").join("ModuleCache");
    fs::create_dir_all(&modules).unwrap();
    fs::write(modules.join("module.pcm"), vec![5u8; 8192]).unwrap();
    // This fixture touches the host filesystem, so its path flavor must match
    // the runner even though the catalog entry is offered only on macOS.
    let environment = PlatformEnvironment::simulated(PathFlavor::current()).with_roots(Arc::new(
        SimulatedPaths::new()
            .with_home(fixture.path())
            .with_user_cache_dir(&cache_root),
    ));
    let registry = SignatureRegistry::load_embedded_catalog().unwrap();
    let signature = registry.get("dev.clang.module_cache").unwrap();
    assert_eq!(signature.paths, ["${DARWIN_USER_CACHE}/clang"]);
    assert_eq!(
        signature.platforms,
        [neati_lib::models::PlatformKind::Macos]
    );
    assert_eq!(
        SignatureLoader::expand_path(&signature.paths[0], &environment),
        Some(cache_root.join("clang")),
        "the simulated cache root must resolve to the real fixture on every runner"
    );
    assert_eq!(signature.min_age_days, Some(0));
    assert_eq!(signature.strategy, CleanStrategy::DeleteContents);
    assert_eq!(signature.risk, RiskTier::Rebuild);
    assert_eq!(
        signature.deletion_disposition,
        Some(neati_core::domain::cleanup::DeletionDisposition::PermanentDelete)
    );
    for owner in [
        "Xcode",
        "xcodebuild",
        "xctest",
        "XCTRunner",
        "XCBBuildService",
        "swift-frontend",
        "clang",
        "clangd",
        "swiftc",
        "sourcekit-lsp",
        "SourceKitService",
    ] {
        assert!(signature.fail_if_running.iter().any(|guard| guard == owner));
    }
    let items = DirectoryScanner::scan_signature(
        signature,
        &environment,
        &NeverCancelled,
        &neati_lib::applications::RunningApplications::from_process_names(["fixture-idle".into()])
            .with_open_file_probe(std::sync::Arc::new(
                neati_platform::open_files::FixedOpenFileProbe(
                    neati_platform::open_files::OpenFileState::Idle,
                ),
            )),
    );
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].path, modules.to_string_lossy());
    assert!(
        items[0].cleanable_bytes() > 0,
        "recent compiler output is measurable"
    );
    let broad = registry
        .get("system.intensive.darwin_user_cache_other")
        .unwrap();
    assert!(broad
        .exclude_prefixes
        .iter()
        .any(|prefix| prefix == "clang"));
    assert!(DirectoryScanner::scan_signature(
        broad,
        &environment,
        &NeverCancelled,
        &neati_lib::applications::RunningApplications::from_process_names(["fixture-idle".into()])
            .with_open_file_probe(std::sync::Arc::new(
                neati_platform::open_files::FixedOpenFileProbe(
                    neati_platform::open_files::OpenFileState::Idle
                )
            ))
    )
    .iter()
    .all(|item| !item.allows_cleanup() && !item.is_selected));
}

#[test]
fn explicit_disposition_is_narrow_and_cannot_override_provider_actions() {
    use neati_core::domain::cleanup::DeletionDisposition;
    let registry = SignatureRegistry::load_embedded_catalog().unwrap();
    for id in [
        "dev.clang.module_cache",
        "system.chrome.code_cache",
        "system.brave.code_cache",
    ] {
        let signature = registry.get(id).unwrap();
        assert_eq!(
            signature.deletion_disposition,
            Some(DeletionDisposition::PermanentDelete)
        );
        assert_eq!(signature.risk, RiskTier::Rebuild);
        assert!(!signature.fail_if_running.is_empty());
        signature.validate().unwrap();
        for strategy in [
            CleanStrategy::Manual,
            CleanStrategy::OwnerProvider,
            CleanStrategy::ExternalCommand,
        ] {
            let mut invalid = signature.clone();
            invalid.strategy = strategy;
            assert!(invalid
                .validate()
                .unwrap_err()
                .to_string()
                .contains("deletion_disposition"));
        }
    }
    assert_eq!(
        registry
            .get("dev.xcode.derived_data")
            .unwrap()
            .deletion_disposition,
        None
    );
    assert_eq!(
        registry
            .get("system.chrome.http_cache")
            .unwrap()
            .deletion_disposition,
        None
    );
}

#[test]
fn browser_profiles_and_offline_state_are_outside_cleanup_patterns() {
    let registry = SignatureRegistry::load_embedded_catalog().unwrap();
    for signature_id in [
        "system.app_support_cache_segments",
        "system.windows.roaming_cache_segments",
        "system.intensive.windows_browser_caches",
    ] {
        let signature = registry.get(signature_id).unwrap();
        let patterns = signature.paths.join("\n").to_ascii_lowercase();
        for protected in [
            "service worker",
            "cachestorage",
            "cookies",
            "history",
            "bookmarks",
            "login data",
            "local storage",
            "extensions",
            "downloads",
        ] {
            assert!(
                !patterns.contains(protected),
                "{signature_id} must not select browser state `{protected}`"
            );
        }
    }

    let saved_state = registry.get("system.saved_application_state").unwrap();
    assert_eq!(saved_state.strategy, CleanStrategy::Manual);
    assert_eq!(saved_state.risk, RiskTier::Manual);
}

#[test]
fn new_user_space_providers_never_claim_system_ownership() {
    let registry = SignatureRegistry::load_embedded_catalog().unwrap();
    for signature_id in [
        "dev.pip.cache",
        "dev.nuget.http",
        "dev.nuget.temp",
        "dev.nuget.plugins",
        "dev.nuget.global_packages",
    ] {
        let signature = registry.get(signature_id).unwrap();
        assert_eq!(signature.strategy, CleanStrategy::ExternalCommand);
        assert_eq!(signature.family, CleanerFamily::PackageManagers);
        assert!(signature.paths.is_empty());
    }

    let playwright = registry.get("dev.playwright.browsers").unwrap();
    assert_eq!(playwright.strategy, CleanStrategy::Manual);
    assert_eq!(playwright.risk, RiskTier::Manual);
    assert_eq!(playwright.family, CleanerFamily::PackageManagers);
}

#[test]
fn additional_ecosystem_stores_are_measured_but_never_authorized() {
    let fixture = tempfile::tempdir().unwrap();
    let environment =
        PlatformEnvironment::simulated(PathFlavor::current()).with_home(fixture.path());
    let registry = SignatureRegistry::load_embedded_catalog().unwrap();
    for (id, relative) in [
        ("dev.poetry.cache", "Library/Caches/pypoetry/artifacts"),
        ("dev.ruby.downloads", ".gem/ruby/3.4.0/cache"),
        ("dev.hex.cache", ".hex/cache"),
        ("dev.opam.downloads", ".opam/download-cache"),
        ("dev.zig.global_cache", ".cache/zig"),
        ("dev.ruff.global_cache", ".cache/ruff"),
        ("dev.mypy.global_cache", ".cache/mypy"),
    ] {
        let signature = registry.get(id).unwrap();
        signature.validate().unwrap();
        let root = fixture.path().join(relative);
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("payload.bin"), vec![3; 8192]).unwrap();
        let items = DirectoryScanner::scan_signature(
            signature,
            &environment,
            &NeverCancelled,
            &neati_lib::applications::RunningApplications::from_process_names([
                "fixture-idle".into()
            ]),
        );
        // The directory scanner also returns absent-root observations; the
        // engine filters those before publishing its inventory.
        assert_eq!(
            items.iter().map(|item| item.size.logical).sum::<u64>(),
            8192,
            "{id}"
        );
        let measured = items
            .iter()
            .find(|item| {
                // PathBuf::join preserves mixed separators on Windows while
                // signature expansion normalizes them. Compare path semantics,
                // not the two display spellings.
                neati_platform::path_algebra::equal(
                    &item.path,
                    &root.to_string_lossy(),
                    environment.flavor(),
                )
            })
            .unwrap_or_else(|| panic!("{id}: populated store {root:?} missing from {items:?}"));
        assert_eq!(measured.size.logical, 8192, "{id}");
        assert!(
            items
                .iter()
                .all(|item| item.cleanable_bytes() == 0 && !item.is_selected),
            "{id}"
        );
        assert_eq!(signature.strategy, CleanStrategy::Manual);
    }
    let poetry = registry.get("dev.poetry.cache").unwrap();
    assert!(poetry
        .paths
        .iter()
        .all(|path| path.ends_with("/artifacts") || path.ends_with("/cache")));
    let mix = fixture.path().join(".mix/archives/tool.ez");
    fs::create_dir_all(mix.parent().unwrap()).unwrap();
    fs::write(&mix, "installed tool").unwrap();
    assert!(registry
        .get("dev.hex.cache")
        .unwrap()
        .paths
        .iter()
        .all(|path| !path.contains(".mix")));
    assert_eq!(fs::read_to_string(mix).unwrap(), "installed tool");
}

#[test]
fn mixed_ai_tool_roots_are_advisory() {
    let registry = SignatureRegistry::load_embedded_catalog().unwrap();
    for signature_id in [
        "ai.claude.logs",
        "ai.claude.cache",
        "ai.cursor.logs",
        "ai.cursor.extensions.cache",
        "ai.gemini.cache",
        "ai.gemini.logs",
        "ai.codex.cache",
        "ai.aider.cache",
        "ai.opencode.cache",
    ] {
        let signature = registry.get(signature_id).unwrap();
        assert_eq!(signature.strategy, CleanStrategy::Manual, "{signature_id}");
        assert_eq!(signature.risk, RiskTier::Manual, "{signature_id}");
    }
}

#[test]
fn profile_and_app_data_aliases_measure_cursor_cache_once() {
    let fixture = tempfile::tempdir().unwrap();
    let support = fixture.path().join("Library/Application Support");
    let cache = support.join("Cursor/Cache");
    fs::create_dir_all(&cache).unwrap();
    fs::write(cache.join("payload.bin"), vec![1u8; 8192]).unwrap();
    let environment = PlatformEnvironment::simulated(PathFlavor::current()).with_roots(Arc::new(
        SimulatedPaths::new()
            .with_flavor(PathFlavor::current())
            .with_home(fixture.path())
            .with_roaming_app_data(&support),
    ));
    let registry = SignatureRegistry::load_embedded_catalog().unwrap();
    let mut signature = registry.get("ai.cursor.cache").unwrap().clone();
    signature.paths = vec![
        "~/Library/Application Support/Cursor/Cache".into(),
        "${ROAMING_APP_DATA}/Cursor/Cache".into(),
    ];
    let items = DirectoryScanner::scan_signature(
        &signature,
        &environment,
        &NeverCancelled,
        &neati_lib::applications::RunningApplications::from_process_names(["fixture-idle".into()])
            .with_open_file_probe(std::sync::Arc::new(
                neati_platform::open_files::FixedOpenFileProbe(
                    neati_platform::open_files::OpenFileState::Idle,
                ),
            )),
    );
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].size.logical, 8192);
}

#[test]
fn strategy_is_the_only_catalog_source_of_management_and_size_semantics() {
    let registry = SignatureRegistry::load_embedded_catalog().unwrap();
    let mut signature = registry.get("ai.claude.cache").unwrap().clone();
    for (strategy, management, size) in [
        (
            CleanStrategy::DeleteContents,
            CacheManagementMode::Neati,
            CacheSizeSemantics::PhysicalReclaimable,
        ),
        (
            CleanStrategy::DeleteDirectory,
            CacheManagementMode::Neati,
            CacheSizeSemantics::PhysicalReclaimable,
        ),
        (
            CleanStrategy::DeleteStaleContents,
            CacheManagementMode::Neati,
            CacheSizeSemantics::PhysicalReclaimable,
        ),
        (
            CleanStrategy::Manual,
            CacheManagementMode::Advisory,
            CacheSizeSemantics::Informational,
        ),
        (
            CleanStrategy::ExternalCommand,
            CacheManagementMode::ToolManaged,
            CacheSizeSemantics::ConservativeLowerBound,
        ),
        (
            CleanStrategy::OwnerProvider,
            CacheManagementMode::ToolManaged,
            CacheSizeSemantics::ConservativeLowerBound,
        ),
        (
            CleanStrategy::LifecycleProvider,
            CacheManagementMode::ToolManaged,
            CacheSizeSemantics::ConservativeLowerBound,
        ),
        (
            CleanStrategy::DockerPrune,
            CacheManagementMode::ToolManaged,
            CacheSizeSemantics::ConservativeLowerBound,
        ),
    ] {
        signature.strategy = strategy;
        let metadata = signature.cache_metadata();
        assert_eq!(metadata.management_mode, management);
        assert_eq!(metadata.size_semantics, size);
    }
    assert_eq!(
        registry.get("dev.xcode.archives").unwrap().strategy,
        CleanStrategy::Manual
    );
    let text = r#"
[[signatures]]
id = "test.retired-knob"
name = "Retired knob"
category = "system"
risk = "safe"
strategy = "delete_contents"
paths = ["~/.cache/test"]
management_mode = "advisory"
"#;
    assert!(SignatureLoader::load_str(text)
        .unwrap_err()
        .to_string()
        .contains("unknown field"));
}

#[test]
fn model_weights_are_owned_by_model_inventory_and_old_settings_still_load() {
    let fixture = tempfile::tempdir().unwrap();
    let weights = fixture.path().join(".ollama/models/blobs/weights.bin");
    fs::create_dir_all(weights.parent().unwrap()).unwrap();
    fs::write(&weights, vec![2u8; 8192]).unwrap();
    let registry = SignatureRegistry::load_embedded_catalog().unwrap();
    assert!(registry.by_category(Category::Model).is_empty());
    let environment = PlatformEnvironment::simulated(PathFlavor::current())
        .with_home(fixture.path())
        .with_missing_tool("docker")
        .with_missing_tool("npm")
        .with_missing_tool("pnpm")
        .with_missing_tool("uv");
    let result = ScanEngine::scan(
        &registry,
        &LifecycleProviderRegistry::new(vec![]),
        &OwnerProviderRegistry::new(vec![]),
        None,
        &[],
        false,
        &environment,
        &NeverCancelled,
        |_| {},
    );
    assert!(result
        .categories
        .iter()
        .all(|category| category.category != Category::Model));
    assert!(result
        .categories
        .iter()
        .flat_map(|category| &category.items)
        .all(|item| !item.path.contains(".ollama")));
    assert!(weights.is_file());
    let settings: NeatiSettings = serde_json::from_str(
        r#"{"clean_local_models":true,"theme":"dark","clean_ai_tools":false}"#,
    )
    .unwrap();
    assert_eq!(settings.theme, "dark");
    assert!(!settings.clean_ai_tools);
    assert!(!settings.is_category_clean_enabled(Category::Model));
    assert!(serde_json::to_value(settings)
        .unwrap()
        .get("clean_local_models")
        .is_none());
}
