//! The releases of the `lungo` command: where they are published, and what makes one trusted.
//!
//! The command is published in a release registry, named by its **anchor**. The registry exists
//! because MachineFabric chartered it: `<anchor>/charter.json` states, among the rest, where each
//! product's manifest is, and `<anchor>/charter.json.sig` signs those exact bytes with
//! MachineFabric's prod release key, whatever environment the registry's contents are signed
//! under. Nothing about the registry is read from the shape of its URL: the charter is verified
//! against the charter roots this `lungo` was built with — the chain below, under the `prod`
//! label — and must be the charter of the anchor asked, of a release registry, for this build's
//! environment, and within its own validity. The command's manifest is then read where the charter
//! says, with a signature beside it at the same address and `.sig`. The manifest lists, per
//! channel (`release`, `nightly`), every version with the archive of each platform, its SHA-256
//! digest and its size.
//!
//! Nothing in a manifest is believed until its signature chains to the root keys this `lungo` was
//! built with:
//!
//! 1. the signature file carries one or more **release-key certificates**. A certificate is
//!    trusted when two distinct roots of this build signed its exact bytes, it is for this build's
//!    signing environment, and the clock is inside its validity;
//! 2. the **manifest signature** must verify, under the release key of one of those certificates,
//!    over the exact bytes that were fetched.
//!
//! Every certificate in the file must be valid: one that is not means a publishing mistake or
//! tampering, and reading past it would hide either. Only then is the manifest parsed, and an
//! archive taken from it is still held to the digest and size the manifest gives.
//!
//! A `lungo` that was not built as a registry release has no roots, and so no release it could
//! trust: it has no release line ([`identity`] is `None`).

use lungo_build::{Error, Result};
use serde::Deserialize;
use signed_release::{CharterExpectation, CharterRules, CharterTrust, Formats, KindRule, LocationRule};
use std::collections::BTreeMap;
use std::fmt;
use std::io::Read;

pub use signed_release::{ChainError, CharterError};

/// The command's name in the release registry.
pub const APP_ID: &str = "lungo-cli";
/// The one directory every release archive unpacks to, holding the `lungo` command, its licence
/// and its README.
pub const ARCHIVE_ROOT: &str = "lungo";
const CERTIFICATE_FORMAT: &str = "machinefabric-release-key-cert/1";
const ENVELOPE_FORMAT: &str = "machinefabric-manifest-sig/1";
const CHARTER_FORMAT: &str = "machinefabric-registry-charter/1";
const CHARTER_ENVELOPE_FORMAT: &str = "machinefabric-charter-sig/1";
/// The label a charter's certificate carries: charters are signed by prod for every environment.
const CHARTER_LABEL: &str = "prod";
const MANIFEST_SCHEMA_MAJOR: &str = "5";

/// What a released `lungo` knows about its own release line, baked in by its build (`build.rs`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Identity {
    /// The anchor of the release registry this `lungo` was published in.
    pub registry: String,
    /// The channel it was published on: `release` or `nightly`.
    pub channel: String,
    pub trust: Trust,
    /// MachineFabric's roots, which the registry's charter must chain to.
    pub charter_roots: Vec<String>,
}

/// The roots a release-key certificate is checked against, and the environment it must be for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Trust {
    /// Root public keys, in minisign's base64 form.
    pub roots: Vec<String>,
    /// `prod`, `staging` or `lab`.
    pub environment: String,
}

/// This build's release line, or `None` for a build that is not a registry release.
pub fn identity() -> Option<Identity> {
    option_env!("LUNGO_BAKED_RELEASE_LINE").map(parse_baked)
}

/// The release line as `build.rs` wrote it: the registry's anchor, channel, environment, the roots
/// and the charter roots (each comma-separated), with a space between them. It checked every part
/// before it wrote them, so a value of another shape is not something a build can produce.
fn parse_baked(baked: &str) -> Identity {
    let parts: Vec<&str> = baked.split(' ').collect();
    let [registry, channel, environment, roots, charter_roots] = parts[..] else {
        panic!("this lungo's baked release line has {} parts, not 5: {baked:?}", parts.len());
    };
    Identity {
        registry: registry.to_owned(),
        channel: channel.to_owned(),
        trust: Trust { roots: roots.split(',').map(str::to_owned).collect(), environment: environment.to_owned() },
        charter_roots: charter_roots.split(',').map(str::to_owned).collect(),
    }
}

/// The registry's name for the platform this `lungo` runs on, or `None` where the command is
/// not published.
pub fn platform() -> Option<&'static str> {
    Some(match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => "darwin-arm64",
        ("macos", "x86_64") => "darwin-x86_64",
        ("linux", "x86_64") => "linux-x86_64",
        ("linux", "aarch64") => "linux-arm64",
        ("windows", "x86_64") => "windows-x86_64",
        _ => return None,
    })
}

/// What MachineFabric's charters are: the registries it charters, where each keeps its manifests,
/// and the `fabric` a cartridges registry names. `lungo` reads only release registries; the other
/// kinds are here so that a charter of one is refused for its kind, not for its shape.
pub fn charter_rules() -> CharterRules {
    let location = |name: &str, placeholders: &[&str]| LocationRule {
        name: name.to_owned(),
        placeholders: placeholders.iter().map(|p| (*p).to_owned()).collect(),
    };
    CharterRules {
        format: CHARTER_FORMAT.to_owned(),
        fields: vec!["fabric".to_owned()],
        kinds: vec![
            KindRule {
                name: "cartridges".to_owned(),
                locations: vec![location("manifest", &["registry_version"])],
                urls: vec!["fabric".to_owned()],
            },
            KindRule {
                name: "fabric".to_owned(),
                locations: vec![
                    location("manifest", &["manifest_version"]),
                    location("latest", &[]),
                    location("definition", &["kind", "urn_hash", "version"]),
                ],
                urls: vec![],
            },
            KindRule { name: "releases".to_owned(), locations: vec![location("manifest", &["app_id"])], urls: vec![] },
        ],
        reserved_slugs: vec!["dev".to_owned()],
    }
}

/// `trust` as the chain is verified against: MachineFabric's manifest formats, under the
/// environment this `lungo` was built for.
pub fn content_trust(trust: &Trust) -> std::result::Result<signed_release::Trust, String> {
    let roots: Vec<&str> = trust.roots.iter().map(String::as_str).collect();
    let formats = Formats { certificate: CERTIFICATE_FORMAT.to_owned(), envelope: ENVELOPE_FORMAT.to_owned() };
    signed_release::Trust::new(&roots, &trust.environment, formats)
        .map_err(|e| format!("this lungo's signing roots cannot be used: {e}"))
}

/// MachineFabric's charter roots, as charters are verified against them: under the `prod` label.
pub fn charter_trust(charter_roots: &[String]) -> std::result::Result<CharterTrust, String> {
    let roots: Vec<&str> = charter_roots.iter().map(String::as_str).collect();
    let formats = Formats { certificate: CERTIFICATE_FORMAT.to_owned(), envelope: CHARTER_ENVELOPE_FORMAT.to_owned() };
    signed_release::Trust::new(&roots, CHARTER_LABEL, formats)
        .map(|trust| CharterTrust { trust })
        .map_err(|e| format!("this lungo's charter roots cannot be used: {e}"))
}

/// Checks that `envelope` (the bytes of `charter.json.sig`) authorizes `charter` (the exact bytes
/// of `charter.json`) under `trust`, by MachineFabric's [`charter_rules`], and that it charters the
/// release registry at `anchor` for the `environment` this `lungo` verifies, at the time `now`.
/// Returns the address of the command's manifest, as the charter states it.
pub fn verify_charter(
    charter: &[u8],
    envelope: &[u8],
    trust: &CharterTrust,
    anchor: &str,
    environment: &str,
    now: u64,
) -> std::result::Result<String, CharterError> {
    let expect = CharterExpectation { anchor: Some(anchor), kind: "releases", environment: Some(environment) };
    let verified = signed_release::verify_charter(charter, envelope, trust, &charter_rules(), &expect, now)?;
    // The rules hold a release registry's manifest location to taking `{app_id}` and nothing
    // else, so filling it in cannot fail for a charter that verified.
    Ok(verified
        .charter
        .location("manifest", &[("app_id", APP_ID)])
        .expect("a verified release charter's manifest location takes {app_id}"))
}

/// The form a platform's archive is published in: a `.zip` for Windows, a `.tar.gz` for every
/// other platform.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Archive {
    TarGz,
    Zip,
}

impl Archive {
    pub fn of(platform: &str) -> Archive {
        if platform.starts_with("windows-") { Archive::Zip } else { Archive::TarGz }
    }

    /// The installer type the release manifest records it under.
    fn installer_type(self) -> &'static str {
        match self {
            Archive::TarGz => "targz",
            Archive::Zip => "zip",
        }
    }

    pub fn extension(self) -> &'static str {
        match self {
            Archive::TarGz => ".tar.gz",
            Archive::Zip => ".zip",
        }
    }
}

/// One published archive of the command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
    pub version: String,
    pub archive: Archive,
    pub url: String,
    /// Lower-case hexadecimal.
    pub sha256: String,
    pub size: u64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Manifest {
    schema_version: String,
    app_id: String,
    channels: BTreeMap<String, Channel>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Channel {
    latest_version: Option<String>,
    #[serde(default)]
    versions: BTreeMap<String, Version>,
}

#[derive(Deserialize)]
struct Version {
    platforms: BTreeMap<String, Platform>,
}

#[derive(Deserialize)]
struct Platform {
    installers: Vec<Installer>,
}

#[derive(Deserialize)]
struct Installer {
    #[serde(rename = "type")]
    kind: String,
    path: String,
    sha256: String,
    size: u64,
}

/// The latest release of `channel` for `platform` in a manifest already verified, or `None` when
/// the channel has published nothing yet. A manifest that is not the command's, or whose latest
/// version has no archive for the platform, is an error: the first is the wrong document, and the
/// second is a release this platform was left out of.
pub fn latest(manifest: &[u8], channel: &str, platform: &str) -> Result<Option<Release>> {
    let wrong = |why: String| Error::UpdateUntrusted(why);
    let manifest: Manifest =
        serde_json::from_slice(manifest).map_err(|e| wrong(format!("the release manifest cannot be read: {e}")))?;
    if manifest.schema_version.split('.').next() != Some(MANIFEST_SCHEMA_MAJOR) {
        return Err(wrong(format!(
            "the release manifest is of schema {}; this lungo reads schema {MANIFEST_SCHEMA_MAJOR}",
            manifest.schema_version
        )));
    }
    if manifest.app_id != APP_ID {
        return Err(wrong(format!("the release manifest is {}'s, not {APP_ID}'s", manifest.app_id)));
    }
    let Some(releases) = manifest.channels.get(channel) else {
        return Err(wrong(format!("the release manifest has no {channel} channel")));
    };
    let Some(version) = releases.latest_version.as_deref().filter(|v| !v.is_empty()) else {
        return Ok(None);
    };
    let Some(release) = releases.versions.get(version) else {
        return Err(wrong(format!(
            "the release manifest names {version} as the latest {channel} version and does not list it"
        )));
    };
    let Some(for_platform) = release.platforms.get(platform) else {
        let published: Vec<&str> = release.platforms.keys().map(String::as_str).collect();
        return Err(Error::UpdateUnavailable(format!(
            "lungo {version} is not published for {platform} (it is for {})",
            published.join(", ")
        )));
    };
    let form = Archive::of(platform);
    let Some(archive) = for_platform.installers.iter().find(|i| i.kind == form.installer_type()) else {
        return Err(Error::UpdateUnavailable(format!(
            "lungo {version} for {platform} is published without a {} archive",
            form.extension()
        )));
    };
    if archive.sha256.len() != 64 || !archive.sha256.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(wrong(format!("lungo {version} for {platform} carries {:?} as its SHA-256", archive.sha256)));
    }
    if !archive.path.starts_with("https://") {
        return Err(wrong(format!(
            "lungo {version} for {platform} is listed at {:?}, which is not HTTPS",
            archive.path
        )));
    }
    Ok(Some(Release {
        version: version.to_owned(),
        archive: form,
        url: archive.path.clone(),
        sha256: archive.sha256.to_ascii_lowercase(),
        size: archive.size,
    }))
}

/// Where a release registry's files are read from.
pub trait Source {
    /// The bytes at `url`, or `None` where nothing is published there.
    fn get(&self, url: &str) -> Result<Option<Vec<u8>>>;
}

/// The registry itself, over HTTPS.
pub struct Https;

impl Source for Https {
    fn get(&self, url: &str) -> Result<Option<Vec<u8>>> {
        let failed = |e: &dyn fmt::Display| Error::UpdateUnavailable(format!("cannot download {url}: {e}"));
        // Never from a cache: the manifest and its signature are replaced together, and a stale
        // one of either beside a fresh one of the other reads as a forgery.
        let mut response = match ureq::get(url).header("Cache-Control", "no-cache").call() {
            Ok(response) => response,
            Err(ureq::Error::StatusCode(404)) => return Ok(None),
            Err(e) => return Err(failed(&e)),
        };
        let mut bytes = Vec::new();
        response.body_mut().as_reader().read_to_end(&mut bytes).map_err(|e| failed(&e))?;
        Ok(Some(bytes))
    }
}

/// Fetches the charter of the release registry at `registry` (its anchor) and verifies it against
/// `charter_roots`; then the command's manifest and its signature from where the charter says,
/// verifies the one against the other under `trust`, and returns the latest release of `channel`
/// for `platform`. A registry with no charter is not read at all.
pub fn fetch_latest(
    source: &dyn Source,
    registry: &str,
    trust: &Trust,
    charter_roots: &[String],
    channel: &str,
    platform: &str,
    now: u64,
) -> Result<Option<Release>> {
    let content = content_trust(trust).map_err(Error::UpdateUntrusted)?;
    let charters = charter_trust(charter_roots).map_err(Error::UpdateUntrusted)?;
    let charter_url = format!("{registry}/{}", signed_release::CHARTER_FILE);
    let Some(charter) = source.get(&charter_url)? else {
        return Err(Error::UpdateUntrusted(format!(
            "{registry} has no charter ({charter_url} is missing), so it is not a release registry MachineFabric chartered"
        )));
    };
    let charter_sig_url = format!("{registry}/{}", signed_release::CHARTER_SIG_FILE);
    let Some(charter_sig) = source.get(&charter_sig_url)? else {
        return Err(Error::UpdateUntrusted(format!(
            "{registry} has a charter and no signature for it ({charter_sig_url} is missing)"
        )));
    };
    let url = verify_charter(&charter, &charter_sig, &charters, registry, &trust.environment, now)
        .map_err(|e| Error::UpdateUntrusted(format!("the charter of {registry} was refused: {e}")))?;
    let Some(manifest) = source.get(&url)? else {
        return Err(Error::UpdateUnavailable(format!("no lungo release is published at {registry} yet")));
    };
    let signature_url = format!("{url}.sig");
    let Some(signature) = source.get(&signature_url)? else {
        return Err(Error::UpdateUntrusted(format!(
            "{url} is published without a signature ({signature_url} is missing), so nothing in it is trusted"
        )));
    };
    signed_release::verify_manifest(&signature, &manifest, &content, now)
        .map_err(|e| Error::UpdateUntrusted(format!("{url} is not a release manifest this lungo trusts: {e}")))?;
    latest(&manifest, channel, platform)
}

/// A release line a test signs itself: three roots, a release key they certify, and manifests
/// signed with it, in the form the release registry publishes.
///
/// The formats are written out here, not taken from the code under test: a test that asked the
/// verifier what a certificate looks like would agree with it whatever it said.
#[cfg(test)]
pub(crate) mod testing {
    use ed25519_dalek::{Signer, SigningKey};
    use serde_json::json;

    pub const ISSUED_AT: u64 = 1_700_000_000;
    pub const NOT_AFTER: u64 = ISSUED_AT + 365 * 86_400;
    /// A time inside the validity of [`Line::sign`]'s certificate.
    pub const NOW: u64 = ISSUED_AT + 86_400;

    /// An ed25519 key with a minisign key id.
    pub struct Key(SigningKey, [u8; 8]);

    impl Key {
        /// A key made from `seed`, with `seed` repeated as its key id.
        pub fn new(seed: u8) -> Key {
            Key(SigningKey::from_bytes(&[seed; 32]), [seed; 8])
        }

        pub fn id(&self) -> String {
            self.1.iter().map(|b| format!("{b:02x}")).collect()
        }

        /// The public key in minisign's form: `Ed`, the key id, the ed25519 key, in base64.
        pub fn public(&self) -> String {
            let mut bytes = b"Ed".to_vec();
            bytes.extend_from_slice(&self.1);
            bytes.extend_from_slice(self.0.verifying_key().as_bytes());
            base64(&bytes)
        }

        /// A raw ed25519 signature over `bytes`, in base64.
        pub fn sign(&self, bytes: &[u8]) -> String {
            base64(&self.0.sign(bytes).to_bytes())
        }
    }

    pub fn base64(bytes: &[u8]) -> String {
        const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut out = String::new();
        for chunk in bytes.chunks(3) {
            let n = (chunk[0] as u32) << 16
                | (*chunk.get(1).unwrap_or(&0) as u32) << 8
                | *chunk.get(2).unwrap_or(&0) as u32;
            for (i, shift) in [18, 12, 6, 0].into_iter().enumerate() {
                if i <= chunk.len() {
                    out.push(ALPHABET[(n >> shift) as usize & 63] as char);
                } else {
                    out.push('=');
                }
            }
        }
        out
    }

    /// A release line: roots A, B and C, and a release key.
    pub struct Line {
        pub roots: [Key; 3],
        pub release: Key,
    }

    impl Line {
        pub fn new() -> Line {
            Line { roots: [Key::new(1), Key::new(2), Key::new(3)], release: Key::new(9) }
        }

        pub fn trust(&self, environment: &str) -> super::Trust {
            super::Trust { roots: self.roots.iter().map(Key::public).collect(), environment: environment.to_owned() }
        }

        /// The certificate of `key` for `environment`, as the roots sign it.
        pub fn certificate(key: &Key, environment: &str, issued_at: u64, not_after: u64) -> String {
            json!({
                "format": "machinefabric-release-key-cert/1",
                "release_pubkey": key.public(),
                "key_id": key.id(),
                "environment": environment,
                "issued_at": issued_at,
                "not_after": not_after,
            })
            .to_string()
        }

        /// The signature file of `manifest`: `certificate` signed by `signers`, the manifest
        /// signed by `by`.
        pub fn envelope(manifest: &[u8], certificate: &str, signers: &[&Key], by: &Key) -> Vec<u8> {
            let root_signatures: Vec<_> = signers
                .iter()
                .map(|root| json!({ "root_key_id": root.id(), "signature": root.sign(certificate.as_bytes()) }))
                .collect();
            json!({
                "format": "machinefabric-manifest-sig/1",
                "certificates": [{ "certificate": certificate, "root_signatures": root_signatures }],
                "manifest_signature": { "key_id": by.id(), "signature": by.sign(manifest) },
            })
            .to_string()
            .into_bytes()
        }

        /// `manifest` signed as a publish does it: the release key certified for `environment`
        /// by roots A and B, valid from [`ISSUED_AT`] to [`NOT_AFTER`].
        pub fn sign_for(&self, manifest: &[u8], environment: &str) -> Vec<u8> {
            let certificate = Line::certificate(&self.release, environment, ISSUED_AT, NOT_AFTER);
            Line::envelope(manifest, &certificate, &[&self.roots[0], &self.roots[1]], &self.release)
        }

        /// [`Line::sign_for`] the `prod` environment.
        pub fn sign(&self, manifest: &[u8]) -> Vec<u8> {
            self.sign_for(manifest, "prod")
        }

        /// The roots' public keys, as a build is given them.
        pub fn root_keys(&self) -> Vec<String> {
            self.roots.iter().map(Key::public).collect()
        }

        /// The charter of the `kind` registry at `anchor`, whose contents are signed under
        /// `environment`: its manifest at `<anchor>/{app_id}/manifest`, valid from [`ISSUED_AT`]
        /// with no end of its own.
        pub fn charter(anchor: &str, kind: &str, environment: &str) -> Vec<u8> {
            json!({
                "format": "machinefabric-registry-charter/1",
                "anchor": anchor,
                "kind": kind,
                "slug": "test-releases",
                "environment": environment,
                "locations": { "manifest": format!("{anchor}/{{app_id}}/manifest") },
                "issued_at": ISSUED_AT,
                "not_after": null,
            })
            .to_string()
            .into_bytes()
        }

        /// The signature file of `charter`: `certificate` signed by `signers`, the charter by
        /// `by`.
        pub fn charter_envelope(charter: &[u8], certificate: &str, signers: &[&Key], by: &Key) -> Vec<u8> {
            let root_signatures: Vec<_> = signers
                .iter()
                .map(|root| json!({ "root_key_id": root.id(), "signature": root.sign(certificate.as_bytes()) }))
                .collect();
            json!({
                "format": "machinefabric-charter-sig/1",
                "certificates": [{ "certificate": certificate, "root_signatures": root_signatures }],
                "charter_signature": { "key_id": by.id(), "signature": by.sign(charter) },
            })
            .to_string()
            .into_bytes()
        }

        /// `charter` signed as MachineFabric signs charters: the release key certified for
        /// `prod` by roots A and B.
        pub fn sign_charter(&self, charter: &[u8]) -> Vec<u8> {
            let certificate = Line::certificate(&self.release, "prod", ISSUED_AT, NOT_AFTER);
            Line::charter_envelope(charter, &certificate, &[&self.roots[0], &self.roots[1]], &self.release)
        }
    }

    /// The command's manifest with one version on `channel` for `platform`, and the other
    /// channel empty.
    pub fn manifest(channel: &str, version: &str, platform: &str, url: &str, sha256: &str, size: u64) -> Vec<u8> {
        let other = if channel == "release" { "nightly" } else { "release" };
        let kind = if url.ends_with(".zip") { "zip" } else { "targz" };
        json!({
            "schemaVersion": "5.0",
            "appId": "lungo-cli",
            "name": "lungo",
            "lastUpdated": "2026-10-13T00:00:00Z",
            "channels": {
                channel: { "latestVersion": version, "versions": { version: {
                    "publishedAt": "2026-10-13T00:00:00Z",
                    "platforms": { platform: { "label": platform, "installers": [
                        { "type": kind, "label": "archive", "name": url.rsplit('/').next().unwrap(), "path": url, "sha256": sha256, "size": size }
                    ] } },
                } } },
                other: { "latestVersion": "", "versions": {} },
            },
        })
        .to_string()
        .into_bytes()
    }
}

#[cfg(test)]
mod tests {
    use super::testing::*;
    use super::*;

    const SHA: &str = "b74a72297e9404fc35470af7fc77f157e2899577d2907bcdffe90397f89c14a5";
    const URL: &str = "https://r.example/lungo-cli/release/1.200.0/lungo-1.200.0-linux-x86_64.tar.gz";

    fn a_manifest() -> Vec<u8> {
        manifest("release", "1.200.0", "linux-x86_64", URL, SHA, 4096)
    }

    /// The manifest's chain verified as a published `lungo` verifies it.
    fn verify_manifest(manifest: &[u8], envelope: &[u8], trust: &Trust, now: u64) -> std::result::Result<(), ChainError> {
        signed_release::verify_manifest(envelope, manifest, &content_trust(trust).unwrap(), now).map(|_| ())
    }

    fn chain(manifest: &[u8], envelope: &[u8], line: &Line) -> std::result::Result<(), ChainError> {
        verify_manifest(manifest, envelope, &line.trust("prod"), NOW)
    }

    fn charters(roots: &[String]) -> CharterTrust {
        charter_trust(roots).unwrap()
    }

    /// TEST0349: the baked release line is read back as it was given
    #[test]
    fn test0349_the_baked_release_line_is_read_back_as_it_was_given() {
        let line = parse_baked("https://release.example nightly staging RWAAAA,RWBBBB,RWCCCC RWDDDD,RWEEEE,RWFFFF");
        assert_eq!(
            line,
            Identity {
                registry: "https://release.example".into(),
                channel: "nightly".into(),
                trust: Trust {
                    roots: vec!["RWAAAA".into(), "RWBBBB".into(), "RWCCCC".into()],
                    environment: "staging".into()
                },
                charter_roots: vec!["RWDDDD".into(), "RWEEEE".into(), "RWFFFF".into()],
            }
        );
    }

    /// TEST0373: a release registry is read through its charter
    #[test]
    fn test0373_a_release_registry_is_read_through_its_charter() {
        // The manifest is where the charter says — here a path no URL convention would put it
        // at — and the charter is held to the registry asked, to its kind, to this build's
        // environment and to its own validity.
        let line = Line::new();
        let anchor = "https://release.example/exp/t1/releases";
        let roots = line.root_keys();
        let charter_at = |edit: &dyn Fn(&mut serde_json::Value)| {
            let mut body: serde_json::Value =
                serde_json::from_slice(&Line::charter(anchor, "releases", "prod")).unwrap();
            body["locations"]["manifest"] = serde_json::json!(format!("{anchor}/catalogue/{{app_id}}.json"));
            edit(&mut body);
            body.to_string().into_bytes()
        };
        let verify = |charter: &[u8], environment: &str, now: u64| {
            verify_charter(charter, &line.sign_charter(charter), &charters(&roots), anchor, environment, now)
        };
        let charter = charter_at(&|_| {});
        assert_eq!(verify(&charter, "prod", NOW), Ok(format!("{anchor}/catalogue/lungo-cli.json")));

        let other = charter_at(&|b| b["anchor"] = serde_json::json!("https://release.example/elsewhere"));
        assert_eq!(
            verify(&other, "prod", NOW),
            Err(CharterError::AnchorMismatch {
                charter: "https://release.example/elsewhere".into(),
                asked: anchor.into()
            })
        );
        let cartridges = charter_at(&|b| {
            b["kind"] = serde_json::json!("cartridges");
            b["locations"]["manifest"] = serde_json::json!(format!("{anchor}/v{{registry_version}}/manifest"));
            b["fabric"] = serde_json::json!("https://fabric.example");
        });
        assert_eq!(
            verify(&cartridges, "prod", NOW),
            Err(CharterError::KindMismatch { charter: "cartridges".into(), expected: "releases".into() })
        );
        assert_eq!(
            verify(&charter, "staging", NOW),
            Err(CharterError::EnvironmentMismatch { charter: "prod".into(), client: "staging".into() })
        );
        let lab = charter_at(&|b| {
            b["environment"] = serde_json::json!("lab");
            b["experiment"] = serde_json::json!("t1");
            b["not_after"] = serde_json::json!(NOW + 10);
        });
        assert_eq!(verify(&lab, "lab", NOW), Ok(format!("{anchor}/catalogue/lungo-cli.json")));
        assert_eq!(
            verify(&lab, "lab", NOW + 11),
            Err(CharterError::Expired { not_after: NOW + 10, now: NOW + 11 }),
            "a lab charter lapses on its own"
        );
        let later = charter_at(&|b| b["issued_at"] = serde_json::json!(NOW + 100));
        assert_eq!(verify(&later, "prod", NOW), Err(CharterError::NotYetValid { issued_at: NOW + 100, now: NOW }));
        let no_app = charter_at(&|b| b["locations"]["manifest"] = serde_json::json!(format!("{anchor}/manifest")));
        assert!(matches!(verify(&no_app, "prod", NOW), Err(CharterError::Malformed(_))));
        let unknown = charter_at(&|b| b["mirror"] = serde_json::json!("https://elsewhere.example"));
        assert!(
            matches!(verify(&unknown, "prod", NOW), Err(CharterError::Malformed(_))),
            "a field a charter does not have"
        );
    }

    /// TEST0374: a charter is prods and is signed as itself
    #[test]
    fn test0374_a_charter_is_prods_and_is_signed_as_itself() {
        let line = Line::new();
        let anchor = "https://release.example";
        let roots = line.root_keys();
        let charter = Line::charter(anchor, "releases", "staging");
        let verify = |envelope: &[u8], body: &[u8], roots: &[String]| {
            verify_charter(body, envelope, &charters(roots), anchor, "staging", NOW)
        };
        assert!(verify(&line.sign_charter(&charter), &charter, &roots).is_ok());

        let mut changed = charter.clone();
        let at = changed.iter().position(|b| *b == b'r').unwrap();
        changed[at] = b's';
        assert!(matches!(
            verify(&line.sign_charter(&charter), &changed, &roots),
            Err(CharterError::SignatureInvalid(_))
        ));
        // A certificate for staging, by the same roots: charters are signed by prod alone.
        let staging = Line::certificate(&line.release, "staging", ISSUED_AT, NOT_AFTER);
        let envelope = Line::charter_envelope(&charter, &staging, &[&line.roots[0], &line.roots[1]], &line.release);
        assert!(matches!(
            verify(&envelope, &charter, &roots),
            Err(CharterError::Chain(ChainError::LabelMismatch { .. }))
        ));
        // Roots other than the charter roots.
        let strangers = Line { roots: [Key::new(21), Key::new(22), Key::new(23)], release: Key::new(29) };
        assert!(matches!(
            verify(&line.sign_charter(&charter), &charter, &strangers.root_keys()),
            Err(CharterError::Chain(ChainError::InsufficientRootSignatures { .. }))
        ));
        // A prod-signed manifest signature over the same bytes is not a charter's.
        assert_eq!(
            verify(&line.sign(&charter), &charter, &roots),
            Err(CharterError::Chain(ChainError::UnsupportedEnvelopeFormat {
                found: ENVELOPE_FORMAT.into(),
                expected: CHARTER_ENVELOPE_FORMAT.into()
            }))
        );
    }

    /// TEST0350: a manifest signed by a certified release key is trusted
    #[test]
    fn test0350_a_manifest_signed_by_a_certified_release_key_is_trusted() {
        let line = Line::new();
        let manifest = a_manifest();
        assert_eq!(chain(&manifest, &line.sign(&manifest), &line), Ok(()));
    }

    /// TEST0351: one changed byte of the manifest is refused
    #[test]
    fn test0351_one_changed_byte_of_the_manifest_is_refused() {
        let line = Line::new();
        let manifest = a_manifest();
        let envelope = line.sign(&manifest);
        let mut other = manifest.clone();
        let at = other.iter().position(|b| *b == b'0').unwrap();
        other[at] = b'1';
        assert!(matches!(chain(&other, &envelope, &line), Err(ChainError::SignatureInvalid(_))));
    }

    /// TEST0352: a certificate needs two distinct roots of this build
    #[test]
    fn test0352_a_certificate_needs_two_distinct_roots_of_this_build() {
        let line = Line::new();
        let manifest = a_manifest();
        let certificate = Line::certificate(&line.release, "prod", ISSUED_AT, NOT_AFTER);
        let signed_by = |signers: &[&Key]| Line::envelope(&manifest, &certificate, signers, &line.release);
        // One root.
        assert_eq!(
            chain(&manifest, &signed_by(&[&line.roots[0]]), &line),
            Err(ChainError::InsufficientRootSignatures { key_id: line.release.id(), have: 1, need: 2 })
        );
        // The same root twice counts once.
        assert_eq!(
            chain(&manifest, &signed_by(&[&line.roots[0], &line.roots[0]]), &line),
            Err(ChainError::InsufficientRootSignatures { key_id: line.release.id(), have: 1, need: 2 })
        );
        // A key that is not a root of this build counts for nothing.
        let stranger = Key::new(7);
        assert_eq!(
            chain(&manifest, &signed_by(&[&line.roots[0], &stranger]), &line),
            Err(ChainError::InsufficientRootSignatures { key_id: line.release.id(), have: 1, need: 2 })
        );
        // Any two of the three do.
        assert_eq!(chain(&manifest, &signed_by(&[&line.roots[1], &line.roots[2]]), &line), Ok(()));
    }

    /// TEST0353: a certificate is for one environment and one span of time
    #[test]
    fn test0353_a_certificate_is_for_one_environment_and_one_span_of_time() {
        let line = Line::new();
        let manifest = a_manifest();
        assert_eq!(
            verify_manifest(&manifest, &line.sign_for(&manifest, "staging"), &line.trust("prod"), NOW),
            Err(ChainError::LabelMismatch {
                key_id: line.release.id(),
                certificate: "staging".into(),
                trusted: "prod".into()
            })
        );
        let envelope = line.sign(&manifest);
        assert_eq!(
            verify_manifest(&manifest, &envelope, &line.trust("prod"), NOT_AFTER + 1),
            Err(ChainError::ExpiredCertificate { key_id: line.release.id(), not_after: NOT_AFTER, now: NOT_AFTER + 1 })
        );
        assert_eq!(verify_manifest(&manifest, &envelope, &line.trust("prod"), NOT_AFTER), Ok(()));
        assert_eq!(
            verify_manifest(&manifest, &envelope, &line.trust("prod"), ISSUED_AT - 1),
            Err(ChainError::NotYetValidCertificate { key_id: line.release.id(), issued_at: ISSUED_AT, now: ISSUED_AT - 1 })
        );
    }

    /// TEST0354: a manifest signed by a key no certificate names is refused
    #[test]
    fn test0354_a_manifest_signed_by_a_key_no_certificate_names_is_refused() {
        let line = Line::new();
        let manifest = a_manifest();
        let certificate = Line::certificate(&line.release, "prod", ISSUED_AT, NOT_AFTER);
        let other = Key::new(8);
        let envelope = Line::envelope(&manifest, &certificate, &[&line.roots[0], &line.roots[1]], &other);
        assert_eq!(chain(&manifest, &envelope, &line), Err(ChainError::NoAuthorizingCertificate { key_id: other.id() }));
    }

    /// TEST0355: a certificate whose stated key id is not its keys is refused
    #[test]
    fn test0355_a_certificate_whose_stated_key_id_is_not_its_keys_is_refused() {
        let line = Line::new();
        let manifest = a_manifest();
        let honest = Line::certificate(&line.release, "prod", ISSUED_AT, NOT_AFTER);
        let edited = honest.replace(&line.release.id(), "0000000000000000");
        let envelope = Line::envelope(&manifest, &edited, &[&line.roots[0], &line.roots[1]], &line.release);
        assert_eq!(
            chain(&manifest, &envelope, &line),
            Err(ChainError::KeyIdMismatch { stated: "0000000000000000".into(), actual: line.release.id() })
        );
    }

    /// TEST0356: the latest release is the channels for this platform
    #[test]
    fn test0356_the_latest_release_is_the_channels_for_this_platform() {
        let release = latest(&a_manifest(), "release", "linux-x86_64").unwrap().unwrap();
        assert_eq!(
            release,
            Release {
                version: "1.200.0".into(),
                archive: Archive::TarGz,
                url: URL.into(),
                sha256: SHA.into(),
                size: 4096
            }
        );
        // The other channel has published nothing: that is an answer, not an error.
        assert_eq!(latest(&a_manifest(), "nightly", "linux-x86_64").unwrap(), None);
        // A platform the release was not built for is said, with the ones it was.
        let e = latest(&a_manifest(), "release", "darwin-arm64").unwrap_err();
        assert_eq!(e.code(), lungo_build::ErrorCode::UpdateUnavailable);
        assert!(e.to_string().contains("not published for darwin-arm64 (it is for linux-x86_64)"), "{e}");
    }

    /// TEST0357: another products manifest is not this commands
    #[test]
    fn test0357_another_products_manifest_is_not_this_commands() {
        let text = String::from_utf8(a_manifest()).unwrap().replace("\"lungo-cli\"", "\"lungo-runtime\"");
        let e = latest(text.as_bytes(), "release", "linux-x86_64").unwrap_err();
        assert_eq!(e.code(), lungo_build::ErrorCode::UpdateUntrusted);
        assert!(e.to_string().contains("lungo-runtime's, not lungo-cli's"), "{e}");
    }

    /// TEST0358: an archive is listed with a digest and over https
    #[test]
    fn test0358_an_archive_is_listed_with_a_digest_and_over_https() {
        let plain = manifest("release", "1.200.0", "linux-x86_64", &URL.replace("https://", "http://"), SHA, 1);
        assert!(latest(&plain, "release", "linux-x86_64").unwrap_err().to_string().contains("not HTTPS"));
        let short = manifest("release", "1.200.0", "linux-x86_64", URL, "abc", 1);
        assert!(latest(&short, "release", "linux-x86_64").unwrap_err().to_string().contains("as its SHA-256"));
    }
}
