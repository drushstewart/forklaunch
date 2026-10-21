//! The TypeScript client SDK depends on the published `@forklaunch/universal-sdk`.
//! Earlier CLI releases vendored its sources into `<modules>/.forklaunch/universal-sdk`
//! with a `workspace:*` override; this module retires that copy.
use super::*;
use std::collections::BTreeMap;

const PACKAGE: &str = "@forklaunch/universal-sdk";
const RECEIPT: &str = ".forklaunch-sources.json";
const MEMBER: &str = ".forklaunch/universal-sdk";

fn published_version() -> &'static str {
    crate::core::package_json::package_json_constants::UNIVERSAL_SDK_VERSION
        .trim_start_matches(['~', '^', '='])
}

/// Formatters must not rewrite CLI-owned sources, or the next update would see
/// them as user edits. New applications ship this ignore; older ones gain it here.
fn ignore_owned_sources(directories: &[&Path]) -> Result<()> {
    for directory in directories {
        let path = directory.join(".prettierignore");
        if !path.is_file() {
            continue;
        }
        let current = read_to_string(&path)?;
        if !current.lines().any(|line| line.trim() == ".forklaunch/") {
            let separator = if current.is_empty() || current.ends_with('\n') { "" } else { "\n" };
            write(&path, format!("{current}{separator}.forklaunch/\n"))?;
        }
    }
    Ok(())
}

fn modules_directory(root: &Path) -> Result<PathBuf> {
    let manifest = read_manifest_value(&root.join(".forklaunch/manifest.toml"))?;
    Ok(root.join(manifest.get("modules_path").and_then(Value::as_str).unwrap_or("src")))
}

pub(super) fn ensure(root: &Path) -> Result<()> {
    let modules = modules_directory(root)?;
    ignore_owned_sources(&[root, modules.as_path()])?;
    migrate(root)
}

/// Files the vendored copy may contain besides its receipt-owned sources:
/// build outputs and installed dependencies.
fn generated_output(relative: &Path) -> bool {
    relative.components().next().is_some_and(|component| {
        matches!(component.as_os_str().to_str(), Some("node_modules" | "lib" | "dist"))
    }) || relative.extension().is_some_and(|extension| extension == "tsbuildinfo")
        || relative == Path::new(RECEIPT)
}

fn unmodified(copy: &Path) -> Result<bool> {
    let Ok(receipt) = read_to_string(copy.join(RECEIPT)) else {
        return Ok(false);
    };
    let Ok(hashes) = serde_json::from_str::<BTreeMap<String, String>>(&receipt) else {
        return Ok(false);
    };
    for (relative, digest) in &hashes {
        let Ok(bytes) = std::fs::read(copy.join(relative)) else {
            return Ok(false);
        };
        if crate::core::forklaunch_state::sha256_hex(bytes) != *digest {
            return Ok(false);
        }
    }
    // A file added beside the owned sources is application work; keep the copy.
    let mut pending = vec![copy.to_path_buf()];
    while let Some(directory) = pending.pop() {
        for entry in std::fs::read_dir(&directory)? {
            let entry = entry?;
            let path = entry.path();
            let relative = path.strip_prefix(copy)?;
            if generated_output(relative) {
                continue;
            }
            if entry.file_type()?.is_dir() {
                pending.push(path);
            } else if !hashes.contains_key(&relative.to_string_lossy().replace('\\', "/")) {
                return Ok(false);
            }
        }
    }
    Ok(true)
}

fn retire_workspace_entry(value: &mut JsonValue, list: &str) -> bool {
    let mut changed = false;
    if let Some(members) = value.get_mut(list).and_then(JsonValue::as_array_mut) {
        let before = members.len();
        members.retain(|member| member.as_str() != Some(MEMBER));
        changed |= members.len() != before;
    }
    if let Some(overrides) = value.get_mut("overrides").and_then(JsonValue::as_object_mut)
        && overrides.get(PACKAGE).and_then(JsonValue::as_str) == Some("workspace:*")
    {
        overrides.insert(PACKAGE.into(), json!(published_version()));
        changed = true;
    }
    changed
}

/// Remove the vendored universal SDK, its workspace membership and its
/// `workspace:*` override. The copy is deleted only when every file matches its
/// `.forklaunch-sources.json` receipt; otherwise it stays and one warning is printed.
pub(crate) fn migrate(root: &Path) -> Result<()> {
    if !root.join(".forklaunch/manifest.toml").is_file() {
        return Ok(());
    }
    let modules = modules_directory(root)?;
    let copy = modules.join(MEMBER);
    if copy.exists() {
        if !unmodified(&copy)? {
            eprintln!(
                "warning: {} differs from the sources ForkLaunch vendored, so it was kept. Applications now depend on the published {PACKAGE}@{}; move your changes elsewhere and delete the directory.",
                copy.display(),
                published_version()
            );
            return Ok(());
        }
        remove_dir_all(&copy)?;
        let parent = modules.join(".forklaunch");
        if std::fs::read_dir(&parent).is_ok_and(|mut entries| entries.next().is_none()) {
            std::fs::remove_dir(parent)?;
        }
    }
    let directories = if root == modules { vec![root.to_path_buf()] } else { vec![root.to_path_buf(), modules.clone()] };
    for directory in directories {
        let workspace_path = directory.join("pnpm-workspace.yaml");
        if workspace_path.is_file() {
            let mut workspace: JsonValue = serde_yml::from_str(&read_to_string(&workspace_path)?)?;
            if retire_workspace_entry(&mut workspace, "packages") {
                write(&workspace_path, serde_yml::to_string(&workspace)?)?;
            }
        }
        let package_path = directory.join("package.json");
        if package_path.is_file() {
            let source = read_to_string(&package_path)?;
            let mut package: JsonValue = serde_json::from_str(&source)?;
            let mut changed = retire_workspace_entry(&mut package, "workspaces");
            if let Some(workspaces) = package.get_mut("workspaces").and_then(|value| value.get_mut("packages")).and_then(JsonValue::as_array_mut) {
                let before = workspaces.len();
                workspaces.retain(|member| member.as_str() != Some(MEMBER));
                changed |= workspaces.len() != before;
            }
            if changed {
                write(&package_path, format!("{}\n", serde_json::to_string_pretty(&package)?))?;
            }
        }
    }
    // Service declaration packages referenced the vendored copy as a workspace package.
    for project in read_manifest_value(&root.join(".forklaunch/manifest.toml"))?
        .get("projects")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let Some(crate_path) = project
            .get("metadata")
            .and_then(|metadata| metadata.get("cratePath"))
            .and_then(Value::as_str)
        else {
            continue;
        };
        let path = root.join(crate_path).join("sdk/typescript/package.json");
        let Ok(source) = read_to_string(&path) else { continue };
        let mut package: JsonValue = serde_json::from_str(&source)?;
        let mut changed = false;
        for section in ["dependencies", "devDependencies", "peerDependencies"] {
            if let Some(dependency) = package.get_mut(section).and_then(|deps| deps.get_mut(PACKAGE))
                && dependency.as_str() == Some("workspace:*")
            {
                *dependency = json!(crate::core::package_json::package_json_constants::UNIVERSAL_SDK_VERSION);
                changed = true;
            }
        }
        // Injection settings only applied to the workspace copy.
        if let Some(metadata) = package.get_mut("dependenciesMeta").and_then(JsonValue::as_object_mut) {
            if metadata.remove(PACKAGE).is_some() {
                changed = true;
            }
            if metadata.is_empty() {
                package.as_object_mut().context("SDK package manifest")?.remove("dependenciesMeta");
            }
        }
        if changed {
            write(&path, format!("{}\n", serde_json::to_string_pretty(&package)?))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app(runtime: &str) -> tempfile::TempDir {
        let root = tempfile::tempdir().unwrap();
        create_dir_all(root.path().join(".forklaunch")).unwrap();
        write(
            root.path().join(".forklaunch/manifest.toml"),
            format!("app_name='demo'\nmodules_path='src'\nruntime='{runtime}'\n[[projects]]\nname='notes'\n[projects.metadata]\ncratePath='src/notes'\n"),
        )
        .unwrap();
        let copy = root.path().join("src/.forklaunch/universal-sdk");
        create_dir_all(copy.join("src")).unwrap();
        create_dir_all(copy.join("lib")).unwrap();
        write(copy.join("index.ts"), "export {};\n").unwrap();
        write(copy.join("src/runtime.ts"), "export const runtime = 1;\n").unwrap();
        write(copy.join("lib/index.js"), "built\n").unwrap();
        let receipt: BTreeMap<_, _> = [("index.ts", "export {};\n"), ("src/runtime.ts", "export const runtime = 1;\n")]
            .into_iter()
            .map(|(path, content)| (path.to_owned(), crate::core::forklaunch_state::sha256_hex(content)))
            .collect();
        write(copy.join(RECEIPT), serde_json::to_string(&receipt).unwrap()).unwrap();
        write(
            root.path().join("src/pnpm-workspace.yaml"),
            "packages:\n- core\n- .forklaunch/universal-sdk\noverrides:\n  '@forklaunch/universal-sdk': workspace:*\n  '@forklaunch/core': 3.0.1\n",
        )
        .unwrap();
        create_dir_all(root.path().join("src/notes/sdk/typescript")).unwrap();
        write(
            root.path().join("src/notes/sdk/typescript/package.json"),
            r#"{"name":"@demo/notes","devDependencies":{"@forklaunch/universal-sdk":"workspace:*","typescript":"5.9.3"},"dependenciesMeta":{"@forklaunch/universal-sdk":{"injected":false}}}"#,
        )
        .unwrap();
        root
    }

    #[test]
    fn unmodified_vendored_copy_is_removed_with_its_workspace_wiring() {
        let root = app("node");
        migrate(root.path()).unwrap();
        assert!(!root.path().join("src/.forklaunch").exists());
        let workspace: JsonValue = serde_yml::from_str(&read_to_string(root.path().join("src/pnpm-workspace.yaml")).unwrap()).unwrap();
        assert_eq!(workspace["packages"], json!(["core"]));
        assert_eq!(workspace["overrides"][PACKAGE], published_version());
        assert_eq!(workspace["overrides"]["@forklaunch/core"], "3.0.1");
        let package: JsonValue = serde_json::from_str(&read_to_string(root.path().join("src/notes/sdk/typescript/package.json")).unwrap()).unwrap();
        assert_eq!(
            package["devDependencies"][PACKAGE],
            crate::core::package_json::package_json_constants::UNIVERSAL_SDK_VERSION
        );
        assert!(package.get("dependenciesMeta").is_none());
        migrate(root.path()).unwrap();
    }

    #[test]
    fn modified_or_extended_vendored_copy_is_kept() {
        for change in ["edit", "add", "receipt"] {
            let root = app("node");
            let copy = root.path().join("src/.forklaunch/universal-sdk");
            match change {
                "edit" => write(copy.join("src/runtime.ts"), "export const runtime = 2;\n").unwrap(),
                "add" => write(copy.join("src/extra.ts"), "export {};\n").unwrap(),
                _ => remove_file(copy.join(RECEIPT)).unwrap(),
            }
            let before = read_to_string(root.path().join("src/pnpm-workspace.yaml")).unwrap();
            migrate(root.path()).unwrap();
            assert!(copy.join("index.ts").is_file(), "{change}");
            assert_eq!(read_to_string(root.path().join("src/pnpm-workspace.yaml")).unwrap(), before, "{change}");
        }
    }

    #[test]
    fn bun_workspaces_drop_the_vendored_member() {
        let root = app("bun");
        remove_file(root.path().join("src/pnpm-workspace.yaml")).unwrap();
        write(
            root.path().join("src/package.json"),
            r#"{"name":"demo","workspaces":["core",".forklaunch/universal-sdk"],"overrides":{"@forklaunch/universal-sdk":"workspace:*"}}"#,
        )
        .unwrap();
        migrate(root.path()).unwrap();
        let package: JsonValue = serde_json::from_str(&read_to_string(root.path().join("src/package.json")).unwrap()).unwrap();
        assert_eq!(package["workspaces"], json!(["core"]));
        assert_eq!(package["overrides"][PACKAGE], published_version());
    }
}
