//! Bounded, metadata-only Library review. This inventory is deliberately not
//! stored as an uninstall inspection and cannot mint a Trash plan.
use super::{AppFsProbe, NativeAppFsProbe};
use crate::models::{
    AppLeftoverClassification, AppLeftoverInventory, AppLeftoverItem, AppRelatedKind,
    ObservationQuality, PlatformKind,
};
use crate::safety::SymlinkGuard;
use neati_platform::PlatformEnvironment;
#[cfg(all(test, not(target_os = "windows")))]
use std::fs;
use std::{
    path::Path,
    time::{Duration, Instant},
};

const MAX_ITEMS: usize = 128;
const MAX_ROOT_ENTRIES: usize = 512;
const MAX_UNIT_ENTRIES: usize = 2_048;
const MAX_DEPTH: usize = 16;
const LIMITATION: &str = "Read-only observations, separate from Cleanup estimates. A missing owner does not prove uninstall: apps may be relocated, portable, on an unavailable volume, or command-line tools. Cleanup is unavailable.";

struct OwnerInventory {
    owners: Vec<(String, Option<String>)>,
    quality: ObservationQuality,
    incomplete_reasons: Vec<String>,
}

pub fn scan(env: &PlatformEnvironment) -> AppLeftoverInventory {
    let began = Instant::now();
    let owners = discover_owners(env, &NativeAppFsProbe, began);
    scan_with(env, &owners, &NativeAppFsProbe, began)
}

fn scan_with<P: AppFsProbe>(
    env: &PlatformEnvironment,
    apps: &OwnerInventory,
    probe: &P,
    began: Instant,
) -> AppLeftoverInventory {
    let mut result = AppLeftoverInventory {
        items: vec![],
        quality: ObservationQuality::Fresh,
        observed_roots: 0,
        skipped_entry_count: 0,
        incomplete_reasons: apps.incomplete_reasons.clone(),
        limitation: LIMITATION.into(),
    };
    if env.platform() != PlatformKind::Macos {
        result.quality = ObservationQuality::Unavailable;
        result
            .incomplete_reasons
            .push("User Library resource review has a macOS adapter only".into());
        return result;
    }
    let Some(home) = env.user_home() else {
        result.quality = ObservationQuality::Unavailable;
        result
            .incomplete_reasons
            .push("User home is unavailable".into());
        return result;
    };
    for (relative, kind) in [
        ("Caches", AppRelatedKind::Cache),
        ("Logs", AppRelatedKind::Log),
        ("Application Support", AppRelatedKind::ApplicationSupport),
        ("Preferences", AppRelatedKind::Preference),
        ("Containers", AppRelatedKind::Container),
        ("Group Containers", AppRelatedKind::GroupContainer),
    ] {
        let root = home.join("Library").join(relative);
        if !root.exists()
            && probe
                .symlink_metadata(&root)
                .is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound)
        {
            continue;
        }
        if SymlinkGuard::validate_anchored_path(&root, env).is_err() {
            gap(
                &mut result,
                format!("Linked or inaccessible resource root: {}", root.display()),
            );
            continue;
        }
        let entries = match probe.read_dir(&root) {
            Ok(entries) => entries,
            Err(error) => {
                gap(
                    &mut result,
                    format!("Could not inspect {}: {error}", root.display()),
                );
                continue;
            }
        };
        result.observed_roots += 1;
        let mut children = vec![];
        for (index, entry) in entries.enumerate() {
            if index >= MAX_ROOT_ENTRIES || began.elapsed() > Duration::from_secs(10) {
                gap(&mut result, "Resource enumeration reached its budget; uninspected namespaces remain unknown".into());
                break;
            }
            match entry {
                Ok(entry) => children.push(entry.path()),
                Err(error) => gap(
                    &mut result,
                    format!("Resource entry could not be inspected: {error}"),
                ),
            }
        }
        children.sort();
        for path in children {
            if result.items.len() >= MAX_ITEMS || began.elapsed() > Duration::from_secs(10) {
                gap(&mut result, "Resource review reached its item/time budget; remaining namespaces are unknown".into());
                break;
            }
            let name = path
                .file_name()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();
            if name.starts_with('.') {
                continue;
            }
            let (logical_size, allocated_size, quality, reason) = measure(&path, env, probe, began);
            if quality == ObservationQuality::Fresh && logical_size == 0 && allocated_size == 0 {
                continue;
            }
            let (classification, owner_names, evidence) = classify(&name, kind, apps, quality);
            if quality != ObservationQuality::Fresh {
                gap(
                    &mut result,
                    reason
                        .clone()
                        .unwrap_or_else(|| "Partial resource measurement".into()),
                );
            }
            result.items.push(AppLeftoverItem {
                id: format!("leftover-{}", uuid::Uuid::new_v4()),
                name,
                display_path: path.to_string_lossy().into_owned(),
                kind,
                classification,
                owner_names,
                evidence,
                logical_size,
                allocated_size,
                quality,
                incomplete_reason: reason,
            });
        }
    }
    if apps.quality != ObservationQuality::Fresh || !result.incomplete_reasons.is_empty() {
        result.quality = ObservationQuality::Partial;
    }
    result.items.sort_by(|a, b| {
        b.allocated_size
            .cmp(&a.allocated_size)
            .then(a.display_path.cmp(&b.display_path))
    });
    result
}

/// Read only bounded public bundle manifests; never walk application payloads
/// or populate the installed-app/uninstall authority stores.
fn discover_owners<P: AppFsProbe>(
    env: &PlatformEnvironment,
    probe: &P,
    began: Instant,
) -> OwnerInventory {
    let mut result = OwnerInventory {
        owners: vec![],
        quality: ObservationQuality::Fresh,
        incomplete_reasons: vec![],
    };
    if env.platform() != PlatformKind::Macos {
        result.quality = ObservationQuality::Unavailable;
        return result;
    }
    #[cfg(target_os = "windows")]
    {
        let _ = (probe, began);
        result.quality = ObservationQuality::Unavailable;
    }
    #[cfg(not(target_os = "windows"))]
    {
        let mut roots = env.program_files().into_iter().collect::<Vec<_>>();
        if let Some(home) = env.user_home() {
            roots.push(home.join("Applications"));
        }
        roots.sort();
        roots.dedup();
        if roots.is_empty() {
            result
                .incomplete_reasons
                .push("Application roots are unavailable".into());
        }
        for root in roots {
            let metadata = match probe.symlink_metadata(&root) {
                Ok(metadata) => metadata,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => {
                    result.incomplete_reasons.push(format!(
                        "Application root unavailable: {}: {error}",
                        root.display()
                    ));
                    continue;
                }
            };
            if !metadata.is_dir() || SymlinkGuard::validate_anchored_path(&root, env).is_err() {
                result.incomplete_reasons.push(format!(
                    "Linked or changed application root was not followed: {}",
                    root.display()
                ));
                continue;
            }
            let entries = match probe.read_dir(&root) {
                Ok(entries) => entries,
                Err(error) => {
                    result.incomplete_reasons.push(format!(
                        "Application root unavailable: {}: {error}",
                        root.display()
                    ));
                    continue;
                }
            };
            for (index, entry) in entries.enumerate() {
                if index >= MAX_ROOT_ENTRIES || began.elapsed() > Duration::from_secs(10) {
                    result
                        .incomplete_reasons
                        .push("Application identity review reached its budget".into());
                    break;
                }
                let path = match entry {
                    Ok(entry) => entry.path(),
                    Err(error) => {
                        result
                            .incomplete_reasons
                            .push(format!("Application entry unavailable: {error}"));
                        continue;
                    }
                };
                if !path
                    .extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("app"))
                {
                    continue;
                }
                let manifest = path.join("Contents/Info.plist");
                let meta = probe.symlink_metadata(&manifest);
                if SymlinkGuard::validate_anchored_path(&manifest, env).is_err()
                    || !meta
                        .as_ref()
                        .is_ok_and(|m| m.is_file() && m.len() <= 262_144)
                {
                    result.incomplete_reasons.push("An application manifest is linked, unreadable or outside the bounded format".into());
                    continue;
                }
                let before = crate::large_files::identity_from_path(&manifest);
                let value = {
                    use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
                    let mut options = std::fs::OpenOptions::new();
                    options
                        .read(true)
                        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
                    options.open(&manifest).and_then(|file| {
                        let opened = file.metadata()?;
                        let expected = meta.as_ref().unwrap();
                        if !opened.is_file()
                            || opened.len() > 262_144
                            || opened.dev() != expected.dev()
                            || opened.ino() != expected.ino()
                            || SymlinkGuard::validate_anchored_path(&manifest, env).is_err()
                        {
                            return Err(std::io::Error::other(
                                "Application manifest changed before reading",
                            ));
                        }
                        // A manifest may grow after metadata was captured;
                        // keep the actual public read bounded too.
                        use std::io::Read;
                        plist::Value::from_reader(std::io::BufReader::new(file.take(262_144)))
                            .map_err(std::io::Error::other)
                    })
                };
                if before.is_none() || before != crate::large_files::identity_from_path(&manifest) {
                    result
                        .incomplete_reasons
                        .push("An application manifest changed during review".into());
                    continue;
                }
                let Some(dict) = value.as_ref().ok().and_then(plist::Value::as_dictionary) else {
                    result
                        .incomplete_reasons
                        .push("An application manifest could not be parsed".into());
                    continue;
                };
                let get = |key: &str| {
                    dict.get(key)
                        .and_then(plist::Value::as_string)
                        .map(str::to_string)
                };
                let name = get("CFBundleDisplayName")
                    .or_else(|| get("CFBundleName"))
                    .unwrap_or_else(|| path.file_stem().unwrap().to_string_lossy().into_owned());
                let identifier = get("CFBundleIdentifier");
                if identifier.is_none() {
                    result.incomplete_reasons.push("An application has no readable bundle identifier; missing owner matches remain uncertain".into());
                }
                result.owners.push((name, identifier));
                if result.incomplete_reasons.len() >= 16 {
                    break;
                }
            }
        }
    }
    if !result.incomplete_reasons.is_empty() {
        result.quality = ObservationQuality::Partial;
    }
    result.incomplete_reasons.truncate(16);
    result
}

fn gap(result: &mut AppLeftoverInventory, reason: String) {
    result.skipped_entry_count = result.skipped_entry_count.saturating_add(1);
    if result.incomplete_reasons.len() < 16 && !result.incomplete_reasons.contains(&reason) {
        result.incomplete_reasons.push(reason);
    }
}

fn classify(
    name: &str,
    kind: AppRelatedKind,
    apps: &OwnerInventory,
    quality: ObservationQuality,
) -> (AppLeftoverClassification, Vec<String>, String) {
    let namespace = name
        .strip_suffix(".plist")
        .unwrap_or(name)
        .to_ascii_lowercase();
    let mut owners = apps
        .owners
        .iter()
        .filter(|(name, identifier)| {
            identifier.as_ref().is_some_and(|id| {
                let id = id.to_ascii_lowercase();
                namespace == id
                    || namespace
                        .strip_prefix(&id)
                        .is_some_and(|rest| rest.starts_with('.'))
            }) || name.eq_ignore_ascii_case(&namespace)
        })
        .map(|(name, _)| name.clone())
        .collect::<Vec<_>>();
    let matching_bundle_count = owners.len();
    owners.sort();
    owners.dedup();
    if matching_bundle_count > 1 || kind == AppRelatedKind::GroupContainer {
        return (AppLeftoverClassification::AmbiguousSharedOwner, owners, "Shared namespace or multiple matching bundles; helpers, extensions and other owners may still use it. Cleanup unavailable.".into());
    }
    if matches!(
        kind,
        AppRelatedKind::ApplicationSupport | AppRelatedKind::Preference | AppRelatedKind::Container
    ) || namespace.starts_with("com.apple.")
        || namespace.starts_with("group.com.apple.")
    {
        return (AppLeftoverClassification::ProtectedState, owners, "May contain settings, credentials, sessions, databases, documents or offline state. Missing an app is not removal authority.".into());
    }
    if owners.len() == 1 {
        return (AppLeftoverClassification::InstalledOwner, owners, "The resource name matches a bundle identifier or display name in the freshly checked application inventory. It may also serve helpers or command-line tools.".into());
    }
    if apps.quality != ObservationQuality::Fresh || quality != ObservationQuality::Fresh {
        return (AppLeftoverClassification::IncompleteInventory, owners, "Application or resource inspection is incomplete; absence cannot establish an owner was removed.".into());
    }
    let bundle_shaped = namespace.split('.').count() >= 3
        && namespace
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-');
    if bundle_shaped {
        (AppLeftoverClassification::PossibleRemovedOwner, owners, "No matching bundle was found in the checked system/user Applications folders. Relocated apps, unavailable volumes, helpers and command-line owners are not excluded. Read-only review.".into())
    } else {
        (AppLeftoverClassification::AmbiguousSharedOwner, owners, "A folder name does not establish an application identity. Its owner may be a tool, shared service or relocated app. Read-only review.".into())
    }
}

fn measure<P: AppFsProbe>(
    root: &Path,
    env: &PlatformEnvironment,
    probe: &P,
    began: Instant,
) -> (u64, u64, ObservationQuality, Option<String>) {
    let mut logical = 0u64;
    let mut allocated = 0u64;
    let mut count = 0usize;
    let mut stack = vec![(root.to_path_buf(), 0usize)];
    let mut reason = None;
    while let Some((path, depth)) = stack.pop() {
        count += 1;
        if count > MAX_UNIT_ENTRIES
            || depth > MAX_DEPTH
            || began.elapsed() > Duration::from_secs(10)
        {
            reason = Some("Measurement budget reached; size is a lower bound".into());
            break;
        }
        let metadata = match probe.symlink_metadata(&path) {
            Ok(m) => m,
            Err(error) => {
                reason = Some(format!("Resource metadata unavailable: {error}"));
                continue;
            }
        };
        if metadata.file_type().is_symlink()
            || SymlinkGuard::validate_anchored_path(&path, env).is_err()
        {
            reason = Some(
                "Linked or changed resource entries were not followed; size is incomplete".into(),
            );
            continue;
        }
        if metadata.is_file() {
            logical = logical.saturating_add(metadata.len());
            #[cfg(unix)]
            {
                use std::os::unix::fs::MetadataExt;
                allocated = allocated.saturating_add(metadata.blocks().saturating_mul(512));
            }
            #[cfg(not(unix))]
            {
                allocated = allocated.saturating_add(metadata.len());
            }
        } else if metadata.is_dir() {
            match probe.read_dir(&path) {
                Ok(entries) => {
                    for entry in entries {
                        if stack.len() + count >= MAX_UNIT_ENTRIES {
                            reason = Some(
                                "Measurement entry budget reached; size is a lower bound".into(),
                            );
                            break;
                        }
                        match entry {
                            Ok(e) => stack.push((e.path(), depth + 1)),
                            Err(error) => {
                                reason = Some(format!("Resource entry unavailable: {error}"))
                            }
                        }
                    }
                }
                Err(error) => reason = Some(format!("Resource directory unavailable: {error}")),
            }
        } else {
            reason = Some("Special resource entries were not inspected".into());
        }
    }
    let quality = if reason.is_none() {
        ObservationQuality::Fresh
    } else if logical > 0 || allocated > 0 {
        ObservationQuality::Partial
    } else {
        ObservationQuality::Unavailable
    };
    (logical, allocated, quality, reason)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn inventory() -> OwnerInventory {
        OwnerInventory {
            owners: vec![],
            quality: ObservationQuality::Fresh,
            incomplete_reasons: vec![],
        }
    }
    #[test]
    fn stated_non_mac_platforms_report_unavailable_without_adjacent_library_results() {
        for platform in [PlatformKind::Windows, PlatformKind::Linux] {
            let env = PlatformEnvironment::simulated(neati_platform::PathFlavor::Posix)
                .with_platform(platform)
                .with_home("/unused-fixture-home");
            let result = scan(&env);
            assert_eq!(result.quality, ObservationQuality::Unavailable);
            assert!(result.items.is_empty());
            assert_eq!(result.observed_roots, 0);
            assert!(result
                .incomplete_reasons
                .iter()
                .any(|reason| reason.contains("macOS adapter only")));
        }
    }
    #[cfg(not(target_os = "windows"))]
    fn fixture_environment(home: &Path) -> PlatformEnvironment {
        use neati_platform::paths::SimulatedPaths;
        use std::sync::Arc;
        PlatformEnvironment::simulated(neati_platform::PathFlavor::Posix)
            .with_platform(PlatformKind::Macos)
            .with_roots(Arc::new(
                SimulatedPaths::new()
                    .with_home(home)
                    .with_program_files(home.join("SystemApplications")),
            ))
    }
    #[cfg(not(target_os = "windows"))]
    fn bundle(home: &Path, relative: &str, name: &str, identifier: &str) {
        let path = home.join(relative).join("Contents");
        fs::create_dir_all(&path).unwrap();
        let mut values = plist::Dictionary::new();
        values.insert("CFBundleName".into(), plist::Value::String(name.into()));
        values.insert(
            "CFBundleIdentifier".into(),
            plist::Value::String(identifier.into()),
        );
        plist::Value::Dictionary(values)
            .to_file_xml(path.join("Info.plist"))
            .unwrap();
    }
    #[cfg(not(target_os = "windows"))]
    #[test]
    fn fresh_public_bundle_identity_handles_renames_channels_helpers_and_newly_installed_owners() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().canonicalize().unwrap();
        let env = fixture_environment(&home);
        let cache = home.join("Library/Caches/com.example.editor.helper");
        fs::create_dir_all(&cache).unwrap();
        fs::write(cache.join("payload"), b"preserve").unwrap();
        let first = scan(&env);
        assert_eq!(
            first.items[0].classification,
            AppLeftoverClassification::PossibleRemovedOwner
        );
        bundle(
            &home,
            "Applications/Renamed.app",
            "New Display Name",
            "com.example.editor",
        );
        let second = scan(&env);
        assert_eq!(
            second.items[0].classification,
            AppLeftoverClassification::InstalledOwner
        );
        assert_eq!(second.items[0].owner_names, ["New Display Name"]);
        assert_ne!(first.items[0].id, second.items[0].id);
        // A beta channel sharing its stable identifier makes ownership shared.
        bundle(
            &home,
            "SystemApplications/Editor Beta.app",
            "Editor Beta",
            "com.example.editor",
        );
        let third = scan(&env);
        assert_eq!(
            third.items[0].classification,
            AppLeftoverClassification::AmbiguousSharedOwner
        );
        // Portable/external bundles are outside the fixed roots and never turn
        // a missing match into proof of uninstall.
        bundle(&home, "Portable/Other.app", "Other", "com.example.portable");
        assert!(third.limitation.contains("unavailable volume"));
        assert!(third.limitation.contains("command-line"));
        assert_eq!(fs::read(cache.join("payload")).unwrap(), b"preserve");
    }
    #[cfg(not(target_os = "windows"))]
    #[test]
    fn inaccessible_roots_and_item_budget_preserve_unknown_and_partial_observations() {
        struct DeniedRoot;
        impl AppFsProbe for DeniedRoot {
            fn read_dir(&self, path: &Path) -> std::io::Result<fs::ReadDir> {
                if path.file_name().is_some_and(|name| name == "Applications") {
                    Err(std::io::Error::from(std::io::ErrorKind::PermissionDenied))
                } else {
                    fs::read_dir(path)
                }
            }
        }
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().canonicalize().unwrap();
        let env = fixture_environment(&home);
        fs::create_dir_all(home.join("Applications")).unwrap();
        let owners = discover_owners(&env, &DeniedRoot, Instant::now());
        assert_eq!(owners.quality, ObservationQuality::Partial);
        assert!(!owners.incomplete_reasons.is_empty());
        for index in 0..MAX_ITEMS + 1 {
            let path = home
                .join("Library/Caches")
                .join(format!("com.fixture.{index:03}"));
            fs::create_dir_all(&path).unwrap();
            fs::write(path.join("payload"), b"observed").unwrap();
        }
        let result = scan_with(&env, &owners, &NativeAppFsProbe, Instant::now());
        assert_eq!(result.items.len(), MAX_ITEMS);
        assert_eq!(result.quality, ObservationQuality::Partial);
        assert!(result
            .items
            .iter()
            .all(|item| item.classification == AppLeftoverClassification::IncompleteInventory));
        assert!(result
            .incomplete_reasons
            .iter()
            .any(|reason| reason.contains("budget")));
    }
    #[cfg(not(target_os = "windows"))]
    #[test]
    fn old_or_forged_observation_ids_cannot_enter_the_installed_uninstall_plan() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().canonicalize().unwrap();
        let env = fixture_environment(&home);
        bundle(
            &home,
            "Applications/Installed.app",
            "Installed",
            "com.example.installed",
        );
        let cache = home.join("Library/Caches/com.example.absent");
        fs::create_dir_all(&cache).unwrap();
        fs::write(cache.join("payload"), b"preserve").unwrap();
        let review = scan(&env);
        let apps = super::super::ApplicationScanner::scan(&env);
        let app_id = apps.records.keys().next().unwrap();
        let inspection = super::super::ApplicationScanner::inspect(&env, &apps, app_id).unwrap();
        for id in [&review.items[0].id, &"leftover-forged".to_string()] {
            assert!(crate::trash_manager::TrashPlanner::from_app_inspection(
                &env,
                &inspection,
                std::slice::from_ref(id)
            )
            .is_err());
        }
        assert!(cache.join("payload").exists());
    }
    #[test]
    fn absent_named_owner_is_only_possible_and_partial_inventory_stays_unknown() {
        let mut apps = inventory();
        assert_eq!(
            classify(
                "com.example.absent",
                AppRelatedKind::Cache,
                &apps,
                ObservationQuality::Fresh
            )
            .0,
            AppLeftoverClassification::PossibleRemovedOwner
        );
        apps.quality = ObservationQuality::Partial;
        assert_eq!(
            classify(
                "com.example.absent",
                AppRelatedKind::Cache,
                &apps,
                ObservationQuality::Fresh
            )
            .0,
            AppLeftoverClassification::IncompleteInventory
        );
        assert_eq!(
            classify(
                "com.example.absent",
                AppRelatedKind::Container,
                &apps,
                ObservationQuality::Fresh
            )
            .0,
            AppLeftoverClassification::ProtectedState
        );
        assert_eq!(
            classify(
                "shared.tool",
                AppRelatedKind::GroupContainer,
                &apps,
                ObservationQuality::Fresh
            )
            .0,
            AppLeftoverClassification::AmbiguousSharedOwner
        );
    }
    #[test]
    fn identically_named_channels_remain_shared_before_display_names_are_deduplicated() {
        for identifiers in [
            ["com.example.editor", "com.example.editor.beta"],
            ["com.example.editor.beta", "com.example.editor.beta"],
        ] {
            let mut apps = inventory();
            apps.owners = identifiers
                .map(|id| ("Editor".into(), Some(id.into())))
                .into();
            let (classification, names, _) = classify(
                "com.example.editor.beta",
                AppRelatedKind::Cache,
                &apps,
                ObservationQuality::Fresh,
            );
            assert_eq!(
                classification,
                AppLeftoverClassification::AmbiguousSharedOwner
            );
            assert_eq!(names, ["Editor"]);
        }
    }
    #[cfg(not(target_os = "windows"))]
    #[test]
    fn review_measures_only_named_user_library_children_and_never_reads_private_contents() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().canonicalize().unwrap();
        let env = PlatformEnvironment::simulated(neati_platform::PathFlavor::Posix)
            .with_platform(PlatformKind::Macos)
            .with_home(&home);
        let payload = home.join("Library/Caches/com.example.absent/response");
        fs::create_dir_all(payload.parent().unwrap()).unwrap();
        fs::write(&payload, b"private sentinel").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&payload, fs::Permissions::from_mode(0o0)).unwrap();
        }
        fs::create_dir_all(home.join("Documents/com.example.other")).unwrap();
        fs::write(home.join("Documents/com.example.other/source"), b"authored").unwrap();
        let result = scan_with(&env, &inventory(), &NativeAppFsProbe, Instant::now());
        assert_eq!(result.items.len(), 1);
        assert_eq!(
            result.items[0].classification,
            AppLeftoverClassification::PossibleRemovedOwner
        );
        assert_eq!(result.items[0].logical_size, 16);
        assert!(result.items[0].id.starts_with("leftover-"));
        assert!(payload.exists());
        assert!(home.join("Documents/com.example.other/source").exists());
    }
    #[cfg(unix)]
    #[test]
    fn links_remain_unknown_without_measuring_outside_the_review_scope() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().canonicalize().unwrap();
        let env = PlatformEnvironment::simulated(neati_platform::PathFlavor::Posix)
            .with_platform(PlatformKind::Macos)
            .with_home(&home);
        fs::create_dir_all(home.join("Library/Caches")).unwrap();
        fs::write(home.join("outside"), b"outside").unwrap();
        std::os::unix::fs::symlink(
            home.join("outside"),
            home.join("Library/Caches/com.example.link"),
        )
        .unwrap();
        let result = scan_with(&env, &inventory(), &NativeAppFsProbe, Instant::now());
        assert_eq!(result.items[0].quality, ObservationQuality::Unavailable);
        assert_eq!(result.items[0].logical_size, 0);
        assert_eq!(
            result.items[0].classification,
            AppLeftoverClassification::IncompleteInventory
        );
    }
}
