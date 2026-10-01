//! One catalog owns recognition, descent boundaries, and allowed relative paths.
//! Order is significant when ecosystems share an artifact directory.
use super::*;

struct Rule {
    discovery_name: Option<&'static str>,
    relative: &'static str,
    kind: DeveloperArtifactKind,
    recognition: Recognition,
}

enum Recognition {
    Marker {
        names: &'static [&'static str],
        extensions: &'static [&'static str],
        ecosystem: DeveloperEcosystem,
        hint: &'static str,
    },
    Custom(fn(&Path, &str) -> Option<ArtifactMatch>),
    Framework {
        dependency: &'static str,
        hint: &'static str,
    },
    // Go's shared cache is discovered only by the separately scoped home adapter.
    GlobalGo,
}

const RULES: &[Rule] = &[
    Rule {
        discovery_name: Some(".svelte-kit"),
        relative: ".svelte-kit",
        kind: DeveloperArtifactKind::SvelteKitOutput,
        recognition: Recognition::Framework {
            dependency: "@sveltejs/kit",
            hint: "SvelteKit regenerates its default .svelte-kit directory during dev/build. Custom outDir configuration is not evaluated.",
        },
    },
    Rule {
        discovery_name: Some(".next"),
        relative: ".next",
        kind: DeveloperArtifactKind::NextOutput,
        recognition: Recognition::Framework {
            dependency: "next",
            hint: "Next.js regenerates its default .next directory during dev/build. Deployment/offline data may also be present; custom distDir configuration is not evaluated.",
        },
    },
    Rule {
        discovery_name: Some("target"),
        relative: "target",
        kind: DeveloperArtifactKind::CargoTarget,
        recognition: Recognition::Marker {
            names: &["Cargo.toml"],
            extensions: &[],
            ecosystem: DeveloperEcosystem::Rust,
            hint: "cargo build",
        },
    },
    Rule {
        discovery_name: Some("target"),
        relative: "target",
        kind: DeveloperArtifactKind::MavenTarget,
        recognition: Recognition::Marker {
            names: &["pom.xml"],
            extensions: &[],
            ecosystem: DeveloperEcosystem::Java,
            hint: "mvn clean package",
        },
    },
    Rule {
        discovery_name: Some("target"),
        relative: "target",
        kind: DeveloperArtifactKind::SbtTarget,
        recognition: Recognition::Marker {
            names: &["build.sbt"],
            extensions: &[],
            ecosystem: DeveloperEcosystem::Scala,
            hint: "sbt compile",
        },
    },
    Rule {
        discovery_name: Some("target"),
        relative: "target",
        kind: DeveloperArtifactKind::ClojureTarget,
        recognition: Recognition::Marker {
            names: &["project.clj", "deps.edn"],
            extensions: &[],
            ecosystem: DeveloperEcosystem::Clojure,
            hint: "clojure -T:build compile",
        },
    },
    Rule {
        discovery_name: Some("node_modules"),
        relative: "node_modules",
        kind: DeveloperArtifactKind::NodeModules,
        recognition: Recognition::Custom(|root, _| recognize_node(root)),
    },
    Rule {
        discovery_name: Some(".venv"),
        relative: ".venv",
        kind: DeveloperArtifactKind::PythonVenv,
        recognition: Recognition::Custom(recognize_python),
    },
    Rule {
        discovery_name: Some("venv"),
        relative: "venv",
        kind: DeveloperArtifactKind::PythonVenv,
        recognition: Recognition::Custom(recognize_python),
    },
    Rule {
        discovery_name: Some("vendor"),
        relative: "vendor",
        kind: DeveloperArtifactKind::ComposerVendor,
        recognition: Recognition::Custom(|root, _| recognize_vendor(root)),
    },
    Rule {
        discovery_name: Some("vendor"),
        relative: "vendor/bundle",
        kind: DeveloperArtifactKind::RubyBundle,
        recognition: Recognition::Custom(|root, _| recognize_vendor(root)),
    },
    Rule {
        discovery_name: Some("build"),
        relative: "build",
        kind: DeveloperArtifactKind::GradleBuild,
        recognition: Recognition::Custom(recognize_build),
    },
    Rule {
        discovery_name: Some("build"),
        relative: "build",
        kind: DeveloperArtifactKind::CMakeBuild,
        recognition: Recognition::Custom(recognize_build),
    },
    Rule {
        discovery_name: Some(".gradle"),
        relative: ".gradle",
        kind: DeveloperArtifactKind::GradleCache,
        recognition: Recognition::Marker {
            names: &["build.gradle.kts"],
            extensions: &[],
            ecosystem: DeveloperEcosystem::Kotlin,
            hint: "./gradlew build",
        },
    },
    Rule {
        discovery_name: Some(".gradle"),
        relative: ".gradle",
        kind: DeveloperArtifactKind::GradleCache,
        recognition: Recognition::Marker {
            names: &["build.gradle"],
            extensions: &[],
            ecosystem: DeveloperEcosystem::Java,
            hint: "./gradlew build",
        },
    },
    Rule {
        discovery_name: Some(".gradle"),
        relative: ".gradle",
        kind: DeveloperArtifactKind::GradleCache,
        recognition: Recognition::Marker {
            names: &["settings.gradle.kts"],
            extensions: &[],
            ecosystem: DeveloperEcosystem::Kotlin,
            hint: "./gradlew build",
        },
    },
    Rule {
        discovery_name: Some(".gradle"),
        relative: ".gradle",
        kind: DeveloperArtifactKind::GradleCache,
        recognition: Recognition::Marker {
            names: &["settings.gradle"],
            extensions: &[],
            ecosystem: DeveloperEcosystem::Java,
            hint: "./gradlew build",
        },
    },
    Rule {
        discovery_name: Some(".gradle"),
        relative: ".gradle",
        kind: DeveloperArtifactKind::GradleCache,
        recognition: Recognition::Marker {
            names: &["gradlew"],
            extensions: &[],
            ecosystem: DeveloperEcosystem::Java,
            hint: "./gradlew build",
        },
    },
    Rule {
        discovery_name: Some("bin"),
        relative: "bin",
        kind: DeveloperArtifactKind::DotnetBin,
        recognition: Recognition::Marker {
            names: &[],
            extensions: &["csproj", "fsproj", "vbproj", "sln"],
            ecosystem: DeveloperEcosystem::Dotnet,
            hint: "dotnet restore",
        },
    },
    Rule {
        discovery_name: Some("obj"),
        relative: "obj",
        kind: DeveloperArtifactKind::DotnetObj,
        recognition: Recognition::Marker {
            names: &[],
            extensions: &["csproj", "fsproj", "vbproj", "sln"],
            ecosystem: DeveloperEcosystem::Dotnet,
            hint: "dotnet restore",
        },
    },
    Rule {
        discovery_name: Some(".build"),
        relative: ".build",
        kind: DeveloperArtifactKind::SwiftBuild,
        recognition: Recognition::Marker {
            names: &["Package.swift"],
            extensions: &[],
            ecosystem: DeveloperEcosystem::Swift,
            hint: "swift build",
        },
    },
    Rule {
        discovery_name: Some(".dart_tool"),
        relative: ".dart_tool",
        kind: DeveloperArtifactKind::FlutterTooling,
        recognition: Recognition::Marker {
            names: &["pubspec.yaml"],
            extensions: &[],
            ecosystem: DeveloperEcosystem::Dart,
            hint: "flutter pub get",
        },
    },
    Rule {
        discovery_name: Some("_build"),
        relative: "_build",
        kind: DeveloperArtifactKind::ElixirBuild,
        recognition: Recognition::Marker {
            names: &["mix.exs"],
            extensions: &[],
            ecosystem: DeveloperEcosystem::Elixir,
            hint: "mix deps.get",
        },
    },
    Rule {
        discovery_name: Some("_build"),
        relative: "_build",
        kind: DeveloperArtifactKind::ErlangBuild,
        recognition: Recognition::Marker {
            names: &["rebar.config"],
            extensions: &[],
            ecosystem: DeveloperEcosystem::Erlang,
            hint: "rebar3 compile",
        },
    },
    Rule {
        discovery_name: Some("deps"),
        relative: "deps",
        kind: DeveloperArtifactKind::ElixirDeps,
        recognition: Recognition::Marker {
            names: &["mix.exs"],
            extensions: &[],
            ecosystem: DeveloperEcosystem::Elixir,
            hint: "mix deps.get",
        },
    },
    Rule {
        discovery_name: Some(".stack-work"),
        relative: ".stack-work",
        kind: DeveloperArtifactKind::HaskellStackWork,
        recognition: Recognition::Marker {
            names: &["stack.yaml", "cabal.project"],
            extensions: &["cabal"],
            ecosystem: DeveloperEcosystem::Haskell,
            hint: "stack build",
        },
    },
    Rule {
        discovery_name: Some("dist-newstyle"),
        relative: "dist-newstyle",
        kind: DeveloperArtifactKind::HaskellDistNewstyle,
        recognition: Recognition::Marker {
            names: &["stack.yaml", "cabal.project"],
            extensions: &["cabal"],
            ecosystem: DeveloperEcosystem::Haskell,
            hint: "cabal build",
        },
    },
    Rule {
        discovery_name: Some(".zig-cache"),
        relative: ".zig-cache",
        kind: DeveloperArtifactKind::ZigCache,
        recognition: Recognition::Marker {
            names: &["build.zig"],
            extensions: &[],
            ecosystem: DeveloperEcosystem::Zig,
            hint: "zig build",
        },
    },
    Rule {
        discovery_name: Some(".terraform"),
        relative: ".terraform",
        kind: DeveloperArtifactKind::TerraformCache,
        recognition: Recognition::Marker {
            names: &[".terraform.lock.hcl"],
            extensions: &["tf"],
            ecosystem: DeveloperEcosystem::Terraform,
            hint: "terraform init",
        },
    },
    Rule {
        discovery_name: None,
        relative: "pkg/mod",
        kind: DeveloperArtifactKind::GoModuleCache,
        recognition: Recognition::GlobalGo,
    },
];

pub(super) fn recognize(root: &Path, name: &str) -> Option<ArtifactMatch> {
    RULES
        .iter()
        .filter(|rule| rule.discovery_name == Some(name))
        .find_map(|rule| {
            let found = match &rule.recognition {
                Recognition::Marker {
                    names,
                    extensions,
                    ecosystem,
                    hint,
                } => {
                    let marker = find_named_marker(root, names)
                        .or_else(|| find_project_extension_marker(root, extensions))?;
                    ArtifactMatch {
                        ecosystem: *ecosystem,
                        kind: rule.kind,
                        project_root: root.to_path_buf(),
                        artifact_relative: PathBuf::from(rule.relative),
                        evidence: vec![marker.file_name()?.to_string_lossy().into_owned()],
                        marker_paths: vec![marker],
                        rebuild_hint: Some((*hint).to_string()),
                    }
                }
                Recognition::Custom(recognize) => recognize(root, name)?,
                Recognition::Framework { dependency, hint } => {
                    let marker = root.join("package.json");
                    let manifest = read_framework_manifest(&marker)?;
                    let direct_dependency = ["dependencies", "devDependencies", "peerDependencies", "optionalDependencies"]
                        .iter()
                        .filter_map(|section| manifest.get(section)?.as_object()?.get(*dependency)?.as_str())
                        .any(|version| !version.trim().is_empty() && !version.trim().starts_with("npm:"));
                    if !direct_dependency { return None; }
                    ArtifactMatch {
                        ecosystem: DeveloperEcosystem::Node,
                        kind: rule.kind,
                        project_root: root.to_path_buf(),
                        artifact_relative: PathBuf::from(rule.relative),
                        marker_paths: vec![marker],
                        evidence: vec![format!("Direct package.json dependency: {dependency}"), "Default output name; custom configuration was not evaluated. Cleanup is unavailable.".into()],
                        rebuild_hint: Some((*hint).into()),
                    }
                }
                Recognition::GlobalGo => return None,
            };
            // Custom evidence cannot authorize a path or kind absent from its row.
            (found.kind == rule.kind && found.artifact_relative == Path::new(rule.relative))
                .then_some(found)
        })
}

pub(super) fn is_artifact_directory(name: &str) -> bool {
    RULES.iter().any(|rule| {
        rule.discovery_name == Some(name)
            && !matches!(rule.recognition, Recognition::Framework { .. })
    })
}

pub(crate) fn artifact_relative_is_allowed(relative: &Path, kind: DeveloperArtifactKind) -> bool {
    RULES.iter().any(|rule| {
        rule.kind == kind
            && relative == Path::new(rule.relative)
            && !matches!(rule.recognition, Recognition::Framework { .. })
    })
}

pub(crate) fn artifact_is_observation_only(kind: DeveloperArtifactKind) -> bool {
    RULES
        .iter()
        .any(|rule| rule.kind == kind && matches!(rule.recognition, Recognition::Framework { .. }))
}

/// Only bounded regular metadata is read; no configuration or project script runs.
fn read_framework_manifest(path: &Path) -> Option<serde_json::Value> {
    use std::io::Read;
    const MAX_MANIFEST_BYTES: u64 = 256 * 1024;
    let original = fs::symlink_metadata(path).ok()?;
    if !original.is_file() || SymlinkGuard::is_symlink(path) || original.len() > MAX_MANIFEST_BYTES
    {
        return None;
    }
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.custom_flags(0x0020_0000);
    }
    let file = options.open(path).ok()?;
    let opened = file.metadata().ok()?;
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if opened.file_attributes() & 0x400 != 0 {
            return None;
        }
    }
    #[cfg(unix)]
    {
        if original.dev() != opened.dev() || original.ino() != opened.ino() {
            return None;
        }
    }
    if !opened.is_file()
        || original.len() != opened.len()
        || original.modified().ok()? != opened.modified().ok()?
    {
        return None;
    }
    let mut bytes = Vec::new();
    file.take(MAX_MANIFEST_BYTES + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.len() as u64 > MAX_MANIFEST_BYTES {
        return None;
    }
    let current = fs::symlink_metadata(path).ok()?;
    #[cfg(unix)]
    {
        if original.dev() != current.dev() || original.ino() != current.ino() {
            return None;
        }
    }
    if SymlinkGuard::is_symlink(path)
        || original.len() != current.len()
        || original.modified().ok()? != current.modified().ok()?
    {
        return None;
    }
    let value: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    value.as_object()?;
    Some(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marker_rules_require_direct_evidence_and_authorize_only_their_relative_path() {
        for (rule, names, extensions, ecosystem, hint) in
            RULES.iter().filter_map(|rule| match &rule.recognition {
                Recognition::Marker {
                    names,
                    extensions,
                    ecosystem,
                    hint,
                } => Some((rule, names, extensions, ecosystem, hint)),
                _ => None,
            })
        {
            for marker_name in names.iter().map(|name| (*name).to_string()).chain(
                extensions
                    .iter()
                    .map(|extension| format!("project.{extension}")),
            ) {
                let temp = tempfile::tempdir().unwrap();
                let root = temp.path().join("project");
                let sibling = temp.path().join("sibling");
                fs::create_dir_all(root.join(rule.relative)).unwrap();
                fs::create_dir_all(&sibling).unwrap();
                fs::write(temp.path().join(&marker_name), "ancestor").unwrap();
                fs::write(sibling.join(&marker_name), "sibling").unwrap();
                let name = rule.discovery_name.unwrap();
                assert!(recognize(&root, name).is_none(), "{name}: {marker_name}");
                fs::write(root.join(&marker_name), "direct").unwrap();
                let found = recognize(&root, name).expect("direct marker authorizes rule");
                assert_eq!(found.kind, rule.kind);
                assert_eq!(found.ecosystem, *ecosystem);
                assert_eq!(found.rebuild_hint.as_deref(), Some(*hint));
                assert_eq!(found.marker_paths, vec![root.join(&marker_name)]);
                assert!(is_artifact_directory(name));
                assert!(artifact_relative_is_allowed(
                    &found.artifact_relative,
                    found.kind
                ));
                assert!(!artifact_relative_is_allowed(
                    &PathBuf::from("other").join(rule.relative),
                    found.kind
                ));
                assert!(!artifact_relative_is_allowed(
                    &PathBuf::from(rule.relative).join("child"),
                    found.kind
                ));
            }
        }
    }

    #[test]
    fn shared_go_cache_is_not_a_generic_project_or_descent_rule() {
        let temp = tempfile::tempdir().unwrap();
        fs::create_dir_all(temp.path().join("pkg/mod")).unwrap();
        fs::write(temp.path().join("go.mod"), "module fixture").unwrap();
        assert!(recognize(temp.path(), "pkg").is_none());
        assert!(!is_artifact_directory("pkg"));
        assert!(artifact_relative_is_allowed(
            Path::new("pkg/mod"),
            DeveloperArtifactKind::GoModuleCache
        ));
        assert!(!artifact_relative_is_allowed(
            Path::new("pkg"),
            DeveloperArtifactKind::GoModuleCache
        ));
        assert!(!artifact_relative_is_allowed(
            Path::new("target"),
            DeveloperArtifactKind::NodeModules
        ));
    }

    #[test]
    fn framework_defaults_require_exact_direct_bounded_dependency_metadata() {
        for (name, dependency, kind) in [
            (
                ".svelte-kit",
                "@sveltejs/kit",
                DeveloperArtifactKind::SvelteKitOutput,
            ),
            (".next", "next", DeveloperArtifactKind::NextOutput),
        ] {
            let temp = tempfile::tempdir().unwrap();
            let root = temp.path().join("project");
            fs::create_dir_all(root.join(name)).unwrap();
            fs::write(
                temp.path().join("package.json"),
                format!(r#"{{"dependencies":{{"{dependency}":"1.0.0"}}}}"#),
            )
            .unwrap();
            assert!(recognize(&root, name).is_none());
            assert!(
                !is_artifact_directory(name),
                "unverified wrappers remain traversable"
            );
            fs::write(
                root.join("package.json"),
                r#"{"devDependencies":{"svelte":"5","vite":"6"}}"#,
            )
            .unwrap();
            assert!(recognize(&root, name).is_none());
            fs::write(
                root.join("package.json"),
                format!(r#"{{"devDependencies":{{"{dependency}":"^1.0.0"}}}}"#),
            )
            .unwrap();
            let recognized = recognize(&root, name).unwrap();
            assert_eq!(recognized.kind, kind);
            assert_eq!(recognized.marker_paths, vec![root.join("package.json")]);
            assert!(artifact_is_observation_only(kind));
            assert!(!artifact_relative_is_allowed(Path::new(name), kind));
            assert!(recognize(&root, "custom-output").is_none());
            for alias in ["npm:unrelated@1", " npm:unrelated@1"] {
                fs::write(
                    root.join("package.json"),
                    format!(r#"{{"dependencies":{{"{dependency}":"{alias}"}}}}"#),
                )
                .unwrap();
                assert!(recognize(&root, name).is_none());
            }
            fs::write(root.join("package.json"), "{broken json").unwrap();
            assert!(recognize(&root, name).is_none());
            fs::write(root.join("package.json"), [b' '; 256 * 1024 + 1]).unwrap();
            assert!(recognize(&root, name).is_none());
        }
    }

    #[cfg(unix)]
    #[test]
    fn linked_framework_manifest_is_never_read_or_recognized() {
        use std::os::unix::fs::symlink;
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("project");
        fs::create_dir_all(root.join(".next")).unwrap();
        let outside = temp.path().join("outside-package.json");
        fs::write(&outside, r#"{"dependencies":{"next":"16"}}"#).unwrap();
        symlink(&outside, root.join("package.json")).unwrap();
        assert!(recognize(&root, ".next").is_none());
        assert_eq!(
            fs::read_to_string(outside).unwrap(),
            r#"{"dependencies":{"next":"16"}}"#
        );
    }
}
