//! `lungo setup` makes a machine ready for Lean: on one without elan it installs the pinned elan
//! into the per-user elan home and the project's pinned toolchain through it; on one with elan it
//! uses that elan. Neither run finds elan on `PATH`.
//!
//! The test starts from an empty home, so it downloads elan and the Lean toolchain the example
//! pins.

mod support;

use std::path::{Path, PathBuf};
use std::process::Command;
use support::fresh;

/// The Lean release the examples pin.
fn pin() -> String {
    let example = Path::new(env!("CARGO_MANIFEST_DIR")).ancestors().nth(2).unwrap().join("examples/session/lean");
    lungo_driver::read_pin(&example).unwrap()
}

/// A Lake project with no dependencies, pinning `pin`.
fn project(dir: &Path, pin: &str) {
    std::fs::create_dir_all(dir).unwrap();
    std::fs::write(dir.join("lean-toolchain"), format!("{pin}\n")).unwrap();
    std::fs::write(
        dir.join("lakefile.toml"),
        "name = \"hello\"\nversion = \"0.1.0\"\ndefaultTargets = [\"Hello\"]\n\n[[lean_lib]]\nname = \"Hello\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("lake-manifest.json"),
        r#"{"version": "1.2.0", "packagesDir": ".lake/packages", "packages": [], "name": "hello", "lakeDir": ".lake", "fixedToolchain": false}"#,
    )
    .unwrap();
    std::fs::write(dir.join("Hello.lean"), "def hello : Nat := 1\n").unwrap();
}

/// This process's `PATH` without any directory that holds an `elan`.
fn path_without_elan() -> std::ffi::OsString {
    let elan = format!("elan{}", std::env::consts::EXE_SUFFIX);
    let dirs: Vec<PathBuf> = std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
        .filter(|d| !d.join(&elan).exists())
        .collect();
    std::env::join_paths(dirs).unwrap()
}

/// Runs `lungo --project <project> setup` with `home` as the user's home and no `ELAN_HOME`;
/// returns its standard output, failing the test unless it succeeds.
#[track_caller]
fn setup(root: &Path, home: &Path, project: &Path) -> String {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_lungo"));
    cmd.arg("--project")
        .arg(project)
        .arg("setup")
        .current_dir(root)
        .env_remove("ELAN_HOME")
        .env_remove("ELAN_TOOLCHAIN")
        .env("PATH", path_without_elan())
        .env("LUNGO_CACHE_DIR", root.join("cache"));
    if cfg!(windows) {
        cmd.env("USERPROFILE", home);
    } else {
        cmd.env("HOME", home);
    }
    let out = cmd.output().unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    assert!(
        out.status.success(),
        "lungo setup failed ({}):\n{stdout}\n{}",
        out.status,
        String::from_utf8_lossy(&out.stderr)
    );
    stdout
}

fn modified(path: &Path) -> std::time::SystemTime {
    std::fs::metadata(path).unwrap().modified().unwrap()
}

/// TEST0392: `lungo setup` on a machine without elan installs the pinned elan into the empty
/// per-user elan home and the project's toolchain through it, with no elan on PATH; run again,
/// it uses that elan and installs nothing
#[test]
fn test0392_setup_installs_elan_into_an_empty_home_and_then_uses_it() {
    let root = fresh(&Path::new(env!("CARGO_TARGET_TMPDIR")).join("setup-empty-home"));
    let home = root.join("home");
    std::fs::create_dir_all(&home).unwrap();
    // The user's shell profiles, which setup leaves as they are.
    let profiles = [".profile", ".bashrc", ".bash_profile", ".zshrc", ".zshenv"];
    for name in profiles {
        std::fs::write(home.join(name), format!("# {name} as the user wrote it\n")).unwrap();
    }
    let pin = pin();
    let release = lungo_driver::supported_release(&pin).unwrap();
    let hello = root.join("hello");
    project(&hello, &pin);

    let out = setup(&root, &home, &hello);
    let elan_home = home.join(".elan");
    let elan = elan_home.join("bin").join(format!("elan{}", std::env::consts::EXE_SUFFIX));
    let installed_line = format!(" into {}", elan_home.display());
    let version = out
        .lines()
        .find_map(|l| l.strip_prefix("installed elan ")?.strip_suffix(installed_line.as_str()))
        .unwrap_or_else(|| panic!("setup does not say it installed elan into {}:\n{out}", elan_home.display()));
    let reported = support::run(Command::new(&elan).arg("--version"));
    assert!(reported.starts_with(&format!("elan {version} ")), "{reported}");
    assert!(out.contains(&format!("toolchain {pin} ready")), "{out}");
    let commands = format!("elan, lake and lean are in {}: put it on PATH", elan_home.join("bin").display());
    assert!(out.contains(&commands), "{out}");
    for name in profiles {
        let text = std::fs::read_to_string(home.join(name)).unwrap();
        assert_eq!(text, format!("# {name} as the user wrote it\n"), "setup changed {name}");
    }
    let mut created: Vec<String> =
        std::fs::read_dir(&home).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    created.retain(|n| n != ".elan" && !profiles.contains(&n.as_str()));
    assert!(created.is_empty(), "setup wrote {created:?} into the home");

    // The toolchain is usable: elan's `lean` in the project runs the pinned release.
    let mut lean = Command::new(elan_home.join("bin").join(format!("lean{}", std::env::consts::EXE_SUFFIX)));
    lean.arg("--version").current_dir(&hello).env_remove("ELAN_TOOLCHAIN").env("ELAN_HOME", &elan_home);
    let banner = support::run(&mut lean);
    assert!(banner.contains(&format!("version {}", release.version)), "{banner}");
    assert!(banner.contains(release.githash), "{banner}");

    // With elan there, a second setup uses it and installs nothing.
    let installed = modified(&elan);
    let again = setup(&root, &home, &hello);
    assert!(!again.contains("installed elan"), "{again}");
    assert_eq!(modified(&elan), installed, "elan was installed again");
    assert!(again.contains(&format!("toolchain {pin} ready")), "{again}");
    assert!(again.contains(&commands), "{again}");

    std::fs::remove_dir_all(&root).unwrap();
}
