//! Generator plugins: `lungo-gen-<language>` programs on `PATH`, as `protoc` plugins are.
//!
//! A plugin reads one `GenerateRequest` as JSON on its standard input and writes one
//! `GenerateResponse` as JSON on its standard output. It fails by exiting unsuccessfully (its
//! standard error explains why) or by listing errors in the response.

use lungo_build::codegen::plugin::{GenerateRequest, GenerateResponse};
use lungo_build::{Error, Result};
use std::collections::BTreeMap;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

/// The plugin program of `language`, found on `PATH`.
pub fn find(language: &str) -> Result<PathBuf> {
    let name = format!("lungo-gen-{language}{}", std::env::consts::EXE_SUFFIX);
    let path = std::env::var_os("PATH").unwrap_or_default();
    std::env::split_paths(&path)
        .map(|dir| dir.join(&name))
        .find(|p| p.is_file())
        .ok_or_else(|| {
            Error::Plugin(format!(
                "no generator for `{language}`: it is not built in (rust, c, go, python, swift, ts) and no `{name}` is on PATH"
            ))
        })
}

/// Runs the plugin at `program` on `request`; the generated files.
pub fn run(program: &PathBuf, request: &GenerateRequest) -> Result<BTreeMap<String, String>> {
    let shown = program.display();
    let mut child = Command::new(program)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| Error::Plugin(format!("cannot run the generator plugin {shown}: {e}")))?;
    let input = serde_json::to_vec(request).expect("requests serialize");
    let mut stdin = child.stdin.take().expect("stdin is piped");
    // A plugin that exits without reading its input is reported by its status below.
    let written = stdin.write_all(&input);
    drop(stdin);
    let out = child
        .wait_with_output()
        .map_err(|e| Error::Plugin(format!("cannot run the generator plugin {shown}: {e}")))?;
    let stderr = String::from_utf8_lossy(&out.stderr);
    if !out.status.success() {
        return Err(Error::Plugin(format!("the generator plugin {shown} failed ({}):\n{}", out.status, stderr.trim_end())));
    }
    if let Err(e) = written {
        return Err(Error::Plugin(format!("the generator plugin {shown} did not read its request: {e}")));
    }
    let response: GenerateResponse = serde_json::from_slice(&out.stdout).map_err(|e| {
        Error::Plugin(format!("the generator plugin {shown} wrote a malformed response: {e}\n{}", stderr.trim_end()))
    })?;
    if !response.errors.is_empty() {
        return Err(Error::Plugin(format!(
            "the generator plugin {shown} reported errors:\n{}",
            response.errors.iter().map(|e| format!("  {e}")).collect::<Vec<_>>().join("\n")
        )));
    }
    if response.files.is_empty() {
        return Err(Error::Plugin(format!("the generator plugin {shown} generated no files")));
    }
    Ok(response.files)
}
