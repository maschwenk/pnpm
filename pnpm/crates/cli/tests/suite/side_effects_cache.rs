#![cfg(unix)]

use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_testing_utils::bin::{AddMockedRegistry, CommandTempCwd};
use std::{fs, path::Path, process::Command};

/// Regression for <https://github.com/pnpm/pnpm/issues/12042#issuecomment-4682732058>:
/// a package approved via `allowBuilds` whose lifecycle script produces
/// files not in its tarball (e.g. `bun`'s postinstall downloading a
/// binary) loses that output on a warm frozen reinstall.
///
/// `sideEffectsCache` is on by default, so the first build seeds the
/// cache. On the second frozen install the `is_built` gate skips the
/// rebuild — the cached build output must still be materialized into the
/// freshly linked slot, mirroring pnpm's `getFlatMap` applying the
/// side-effects diff at import time. Without that, the slot is left with
/// only the pristine tarball files and the package is broken at runtime.
#[test]
fn side_effects_materialized_on_warm_frozen_reinstall() {
    assert_side_effects_materialized(false);
}

/// TS: `using side effects cache with nodeLinker=hoisted`
/// (`deps-restorer/test/index.ts:706`).
#[test]
fn side_effects_materialized_on_warm_frozen_reinstall_with_hoisted_linker() {
    assert_side_effects_materialized(true);
}

fn assert_side_effects_materialized(hoisted: bool) {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    // `allowBuilds` in `pnpm-workspace.yaml`, exactly like the report.
    let yaml_path = workspace.join("pnpm-workspace.yaml");
    let mut yaml = fs::read_to_string(&yaml_path).expect("read pnpm-workspace.yaml");
    if !yaml.ends_with('\n') {
        yaml.push('\n');
    }
    yaml.push_str("allowBuilds:\n  '@pnpm.e2e/pre-and-postinstall-scripts-example': true\n");
    if hoisted {
        yaml.push_str("nodeLinker: hoisted\n");
    }
    fs::write(&yaml_path, yaml).expect("write pnpm-workspace.yaml");

    let manifest_path = workspace.join("package.json");
    let package_json = serde_json::json!({
        "dependencies": {
            "@pnpm.e2e/pre-and-postinstall-scripts-example": "1.0.0",
        },
    });
    fs::write(&manifest_path, package_json.to_string()).expect("write package.json");

    // `generated-by-postinstall.js` is written by the package's
    // postinstall and is not part of its tarball, so it only exists if
    // the build ran or its cached output was materialized.
    let postinstall_artifact = if hoisted {
        workspace.join(
            "node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-postinstall.js",
        )
    } else {
        workspace.join(
            "node_modules/.pnpm/@pnpm.e2e+pre-and-postinstall-scripts-example@1.0.0\
             /node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example/generated-by-postinstall.js",
        )
    };

    eprintln!("First install (non-frozen, writes lockfile + populates store)...");
    pacquet.with_arg("install").assert().success();

    eprintln!("Wiping node_modules before the first frozen install...");
    fs::remove_dir_all(workspace.join("node_modules")).expect("remove node_modules");

    eprintln!("Frozen install (builds, writes the side-effects cache)...");
    run_frozen_install(&workspace);
    assert!(postinstall_artifact.exists(), "postinstall must run on the first frozen install");

    eprintln!("Wiping node_modules (keep store + lockfile, like a fresh CI checkout)...");
    fs::remove_dir_all(workspace.join("node_modules")).expect("remove node_modules");

    eprintln!("Frozen reinstall (warm store, hits the is_built gate)...");
    run_frozen_install(&workspace);
    assert!(
        postinstall_artifact.exists(),
        "the cached postinstall output must be materialized after a warm frozen reinstall",
    );

    drop((root, mock_instance));
}

/// TS: `uploading side effects ... original file in the store is not modified`
/// (`deps-installer/test/install/sideEffects.ts`).
///
/// A build script that edits a file the package shipped with must not
/// reach the store copy of that file. With hard links it would: the
/// edit lands in the shared CAS blob, every other package linking the
/// blob sees it, and the next install finds the blob failing its
/// integrity check, downloads the tarball again and re-runs the build
/// instead of using the side-effects cache.
#[test]
fn build_script_edits_do_not_reach_the_store_through_hard_links() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, store_dir, .. } = npmrc_info;

    let yaml_path = workspace.join("pnpm-workspace.yaml");
    let mut yaml = fs::read_to_string(&yaml_path).expect("read pnpm-workspace.yaml");
    if !yaml.ends_with('\n') {
        yaml.push('\n');
    }
    yaml.push_str("allowBuilds:\n  '@pnpm/postinstall-modifies-source': true\n");
    yaml.push_str("packageImportMethod: hardlink\n");
    fs::write(&yaml_path, yaml).expect("write pnpm-workspace.yaml");
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "dependencies": { "@pnpm/postinstall-modifies-source": "1.0.0" },
        })
        .to_string(),
    )
    .expect("write package.json");

    pacquet.with_arg("install").assert().success();

    // The postinstall appends `hello` to the package's empty `empty-file.txt`.
    let installed = workspace.join("node_modules/@pnpm/postinstall-modifies-source/empty-file.txt");
    assert_eq!(fs::read_to_string(&installed).expect("read installed file"), "hello");

    // The CAS blob for empty content (sha512 of ``) must still be empty.
    let empty_blob = store_dir.join(
        "v11/files/cf/83e1357eefb8bdf1542850d66d8007d620e4050b5715dc83f4a921d36ce9ce47d0d13c5d85f\
         2b0ff8318d2877eec2f63b931bd47417a81a538327af927da3e",
    );
    assert_eq!(
        fs::read_to_string(&empty_blob).expect("read the store copy of empty-file.txt"),
        "",
        "the build script must edit a copy, not the store blob",
    );

    drop((root, mock_instance));
}

/// A fresh `pacquet install --frozen-lockfile` against an existing
/// workspace. The registry config lives in the workspace's `.npmrc` /
/// `pnpm-workspace.yaml` and the mock registry is a process-global
/// singleton kept alive by the caller, so this only needs its own
/// command — no extra `CommandTempCwd` / registry.
fn run_frozen_install(workspace: &Path) {
    Command::cargo_bin("pnpm")
        .expect("find the pnpm binary")
        .with_current_dir(workspace)
        .with_args(["install", "--frozen-lockfile"])
        .assert()
        .success();
}
