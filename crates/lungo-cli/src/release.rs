//! The releases of the `lungo` command: where they are published, and what makes one trusted.
//!
//! The command is published in a release registry, as one manifest at
//! `<registry>/lungo-cli/manifest` with a signature beside it at `<registry>/lungo-cli/manifest.sig`.
//! The manifest lists, per channel (`release`, `nightly`), every version with the archive of each
//! platform, its SHA-256 digest and its size.
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

use ed25519_dalek::Verifier;
use lungo_build::{Error, Result};
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::io::Read;

/// The command's name in the release registry.
pub const APP_ID: &str = "lungo-cli";
/// The one directory every release archive unpacks to, holding the `lungo` command, its licence
/// and its README.
pub const ARCHIVE_ROOT: &str = "lungo";
const CERTIFICATE_FORMAT: &str = "machinefabric-release-key-cert/1";
const ENVELOPE_FORMAT: &str = "machinefabric-manifest-sig/1";
const MANIFEST_SCHEMA_MAJOR: &str = "5";
/// Distinct roots that must have signed a certificate.
const REQUIRED_ROOT_SIGNATURES: usize = 2;

/// What a released `lungo` knows about its own release line, baked in by its build (`build.rs`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Identity {
    /// The release registry this `lungo` was published in.
    pub registry: String,
    /// The channel it was published on: `release` or `nightly`.
    pub channel: String,
    pub trust: Trust,
}

/// The roots a release-key certificate is checked against, and the environment it must be for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Trust {
    /// Root public keys, in minisign's base64 form.
    pub roots: Vec<String>,
    /// `prod` or `staging`.
    pub environment: String,
}

/// This build's release line, or `None` for a build that is not a registry release.
pub fn identity() -> Option<Identity> {
    option_env!("LUNGO_BAKED_RELEASE_LINE").map(parse_baked)
}

/// The release line as `build.rs` wrote it: registry, channel, environment and the roots
/// (comma-separated), with a space between them. It checked every part before it wrote them, so
/// a value of another shape is not something a build can produce.
fn parse_baked(baked: &str) -> Identity {
    let parts: Vec<&str> = baked.split(' ').collect();
    let [registry, channel, environment, roots] = parts[..] else {
        panic!("this lungo's baked release line has {} parts, not 4: {baked:?}", parts.len());
    };
    Identity {
        registry: registry.to_owned(),
        channel: channel.to_owned(),
        trust: Trust { roots: roots.split(',').map(str::to_owned).collect(), environment: environment.to_owned() },
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

/// Why a manifest's signature does not chain to this build's roots.
#[derive(Debug, PartialEq, Eq)]
pub enum ChainError {
    MalformedEnvelope(String),
    UnsupportedEnvelopeFormat(String),
    NoCertificates,
    MalformedCertificate(String),
    UnsupportedCertificateFormat(String),
    MalformedKey(String),
    KeyIdMismatch { stated: String, actual: String },
    TooFewRootSignatures { key_id: String, have: usize },
    WrongEnvironment { key_id: String, certificate: String, build: String },
    Expired { key_id: String, not_after: u64, now: u64 },
    NotYetValid { key_id: String, issued_at: u64, now: u64 },
    NoCertificateForSigner { key_id: String },
    ManifestSignatureInvalid(String),
}

impl fmt::Display for ChainError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ChainError::MalformedEnvelope(e) => write!(f, "the signature file is not a manifest signature: {e}"),
            ChainError::UnsupportedEnvelopeFormat(found) => {
                write!(f, "the signature file is of format {found:?}, not {ENVELOPE_FORMAT:?}")
            }
            ChainError::NoCertificates => f.write_str("the signature file carries no release-key certificate"),
            ChainError::MalformedCertificate(e) => write!(f, "a release-key certificate cannot be read: {e}"),
            ChainError::UnsupportedCertificateFormat(found) => {
                write!(f, "a release-key certificate is of format {found:?}, not {CERTIFICATE_FORMAT:?}")
            }
            ChainError::MalformedKey(e) => write!(f, "a key or signature cannot be read: {e}"),
            ChainError::KeyIdMismatch { stated, actual } => {
                write!(f, "a release-key certificate says its key is {stated} and carries key {actual}")
            }
            ChainError::TooFewRootSignatures { key_id, have } => write!(
                f,
                "the certificate of release key {key_id} is signed by {have} of this lungo's roots; {REQUIRED_ROOT_SIGNATURES} must sign"
            ),
            ChainError::WrongEnvironment { key_id, certificate, build } => write!(
                f,
                "the certificate of release key {key_id} is for the {certificate} environment; this lungo was built for {build}"
            ),
            ChainError::Expired { key_id, not_after, now } => {
                write!(f, "the certificate of release key {key_id} expired at {not_after} (it is {now})")
            }
            ChainError::NotYetValid { key_id, issued_at, now } => {
                write!(f, "the certificate of release key {key_id} is valid from {issued_at} (it is {now})")
            }
            ChainError::NoCertificateForSigner { key_id } => {
                write!(f, "the manifest is signed by key {key_id}, which no certificate in the signature file authorizes")
            }
            ChainError::ManifestSignatureInvalid(e) => {
                write!(f, "the manifest's signature does not match the manifest: {e}")
            }
        }
    }
}

#[derive(Deserialize)]
struct Envelope {
    format: String,
    certificates: Vec<CertificateEntry>,
    manifest_signature: ManifestSignature,
}

#[derive(Deserialize)]
struct CertificateEntry {
    /// The certificate as the roots signed it: these exact bytes, never re-serialized.
    certificate: String,
    root_signatures: Vec<RootSignature>,
}

#[derive(Deserialize)]
struct RootSignature {
    signature: String,
}

#[derive(Deserialize)]
struct ManifestSignature {
    key_id: String,
    signature: String,
}

#[derive(Deserialize)]
struct Certificate {
    format: String,
    release_pubkey: String,
    key_id: String,
    environment: String,
    issued_at: u64,
    not_after: u64,
}

/// Checks that `envelope` (the bytes of `manifest.sig`) authorizes `manifest` (the exact bytes
/// of the manifest it was fetched beside) under `trust`, at the time `now` (seconds since 1970).
pub fn verify_manifest(
    manifest: &[u8],
    envelope: &[u8],
    trust: &Trust,
    now: u64,
) -> std::result::Result<(), ChainError> {
    let envelope: Envelope =
        serde_json::from_slice(envelope).map_err(|e| ChainError::MalformedEnvelope(e.to_string()))?;
    if envelope.format != ENVELOPE_FORMAT {
        return Err(ChainError::UnsupportedEnvelopeFormat(envelope.format));
    }
    if envelope.certificates.is_empty() {
        return Err(ChainError::NoCertificates);
    }
    let mut release_keys: Vec<(String, String)> = Vec::new();
    for entry in &envelope.certificates {
        let certificate = verify_certificate(entry, trust, now)?;
        release_keys.push((certificate.key_id, certificate.release_pubkey));
    }
    let signer = &envelope.manifest_signature.key_id;
    let Some((_, release_key)) = release_keys.iter().find(|(id, _)| id == signer) else {
        return Err(ChainError::NoCertificateForSigner { key_id: signer.clone() });
    };
    verify_signature(release_key, &envelope.manifest_signature.signature, manifest).map_err(|e| match e {
        SignatureError::Malformed(why) => ChainError::MalformedKey(why),
        SignatureError::Mismatch(why) => ChainError::ManifestSignatureInvalid(why),
    })
}

fn verify_certificate(
    entry: &CertificateEntry,
    trust: &Trust,
    now: u64,
) -> std::result::Result<Certificate, ChainError> {
    let certificate: Certificate =
        serde_json::from_str(&entry.certificate).map_err(|e| ChainError::MalformedCertificate(e.to_string()))?;
    if certificate.format != CERTIFICATE_FORMAT {
        return Err(ChainError::UnsupportedCertificateFormat(certificate.format));
    }
    // The stated key id must be the key's own: a mismatch is a certificate edited by hand.
    let key = PublicKey::parse(&certificate.release_pubkey).map_err(ChainError::MalformedKey)?;
    if key.id != certificate.key_id {
        return Err(ChainError::KeyIdMismatch { stated: certificate.key_id, actual: key.id });
    }
    // Distinct ROOTS, not distinct signatures: a signature by a key that is not a root counts for
    // nothing, and two by one root count once.
    let signers: BTreeSet<usize> = trust
        .roots
        .iter()
        .enumerate()
        .filter(|(_, root)| {
            entry
                .root_signatures
                .iter()
                .any(|s| verify_signature(root, &s.signature, entry.certificate.as_bytes()).is_ok())
        })
        .map(|(index, _)| index)
        .collect();
    if signers.len() < REQUIRED_ROOT_SIGNATURES {
        return Err(ChainError::TooFewRootSignatures { key_id: certificate.key_id, have: signers.len() });
    }
    if certificate.environment != trust.environment {
        return Err(ChainError::WrongEnvironment {
            key_id: certificate.key_id,
            certificate: certificate.environment,
            build: trust.environment.clone(),
        });
    }
    if now > certificate.not_after {
        return Err(ChainError::Expired { key_id: certificate.key_id, not_after: certificate.not_after, now });
    }
    if certificate.issued_at > now {
        return Err(ChainError::NotYetValid { key_id: certificate.key_id, issued_at: certificate.issued_at, now });
    }
    Ok(certificate)
}

enum SignatureError {
    Malformed(String),
    Mismatch(String),
}

/// A raw ed25519 signature (base64) by a minisign public key (base64), over exact bytes.
fn verify_signature(public_key: &str, signature: &str, bytes: &[u8]) -> std::result::Result<(), SignatureError> {
    let key = PublicKey::parse(public_key).map_err(SignatureError::Malformed)?;
    let key = ed25519_dalek::VerifyingKey::from_bytes(&key.ed25519)
        .map_err(|e| SignatureError::Malformed(e.to_string()))?;
    let signature = base64_decode(signature.trim())
        .ok_or_else(|| SignatureError::Malformed("a signature is not base64".to_owned()))?;
    let signature: [u8; 64] = signature
        .try_into()
        .map_err(|s: Vec<u8>| SignatureError::Malformed(format!("a signature is {} bytes, not 64", s.len())))?;
    key.verify(bytes, &ed25519_dalek::Signature::from_bytes(&signature))
        .map_err(|e| SignatureError::Mismatch(e.to_string()))
}

/// A minisign public key: `Ed`, an 8-byte key id, a 32-byte ed25519 key, in base64.
struct PublicKey {
    /// The key id, lower-case hexadecimal.
    id: String,
    ed25519: [u8; 32],
}

impl PublicKey {
    fn parse(text: &str) -> std::result::Result<PublicKey, String> {
        let bytes = base64_decode(text.trim()).ok_or("a public key is not base64")?;
        if bytes.len() != 42 {
            return Err(format!("a public key is {} bytes, not 42", bytes.len()));
        }
        if &bytes[..2] != b"Ed" {
            return Err("a public key does not start with the ed25519 tag `Ed`".to_owned());
        }
        let mut ed25519 = [0u8; 32];
        ed25519.copy_from_slice(&bytes[10..]);
        Ok(PublicKey { id: bytes[2..10].iter().map(|b| format!("{b:02x}")).collect(), ed25519 })
    }
}

/// Standard base64 with padding, as minisign and the signature files write it. Anything else
/// (a character outside the alphabet, a length that is not a multiple of four, data after the
/// padding, bits left over) is `None`.
fn base64_decode(text: &str) -> Option<Vec<u8>> {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let bytes = text.as_bytes();
    if bytes.is_empty() || !bytes.len().is_multiple_of(4) {
        return None;
    }
    let padding = bytes.iter().rev().take_while(|b| **b == b'=').count();
    if padding > 2 {
        return None;
    }
    let mut out = Vec::with_capacity(bytes.len() / 4 * 3);
    let (mut acc, mut bits) = (0u32, 0u32);
    for byte in &bytes[..bytes.len() - padding] {
        let value = ALPHABET.iter().position(|a| a == byte)? as u32;
        acc = (acc << 6) | value;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    // What is left are the bits the padding stands for, and they are zero in a canonical encoding.
    (acc == 0).then_some(out)
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
        return Err(wrong(format!("lungo {version} for {platform} is listed at {:?}, which is not HTTPS", archive.path)));
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

/// Fetches the command's manifest and its signature from `registry`, verifies the one against
/// the other under `trust`, and returns the latest release of `channel` for `platform`.
pub fn fetch_latest(
    source: &dyn Source,
    registry: &str,
    trust: &Trust,
    channel: &str,
    platform: &str,
    now: u64,
) -> Result<Option<Release>> {
    let url = format!("{}/{APP_ID}/manifest", registry.trim_end_matches('/'));
    let Some(manifest) = source.get(&url)? else {
        return Err(Error::UpdateUnavailable(format!("no lungo release is published at {registry} yet")));
    };
    let signature_url = format!("{url}.sig");
    let Some(signature) = source.get(&signature_url)? else {
        return Err(Error::UpdateUntrusted(format!(
            "{url} is published without a signature ({signature_url} is missing), so nothing in it is trusted"
        )));
    };
    verify_manifest(&manifest, &signature, trust, now)
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

    fn chain(manifest: &[u8], envelope: &[u8], line: &Line) -> std::result::Result<(), ChainError> {
        verify_manifest(manifest, envelope, &line.trust("prod"), NOW)
    }

    /// TEST0349: the baked release line is read back as it was given
    #[test]
    fn test0349_the_baked_release_line_is_read_back_as_it_was_given() {
        let line = parse_baked("https://release.example nightly staging RWAAAA,RWBBBB,RWCCCC");
        assert_eq!(
            line,
            Identity {
                registry: "https://release.example".into(),
                channel: "nightly".into(),
                trust: Trust {
                    roots: vec!["RWAAAA".into(), "RWBBBB".into(), "RWCCCC".into()],
                    environment: "staging".into()
                },
            }
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
        assert!(matches!(chain(&other, &envelope, &line), Err(ChainError::ManifestSignatureInvalid(_))));
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
            Err(ChainError::TooFewRootSignatures { key_id: line.release.id(), have: 1 })
        );
        // The same root twice counts once.
        assert_eq!(
            chain(&manifest, &signed_by(&[&line.roots[0], &line.roots[0]]), &line),
            Err(ChainError::TooFewRootSignatures { key_id: line.release.id(), have: 1 })
        );
        // A key that is not a root of this build counts for nothing.
        let stranger = Key::new(7);
        assert_eq!(
            chain(&manifest, &signed_by(&[&line.roots[0], &stranger]), &line),
            Err(ChainError::TooFewRootSignatures { key_id: line.release.id(), have: 1 })
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
            Err(ChainError::WrongEnvironment {
                key_id: line.release.id(),
                certificate: "staging".into(),
                build: "prod".into()
            })
        );
        let envelope = line.sign(&manifest);
        assert_eq!(
            verify_manifest(&manifest, &envelope, &line.trust("prod"), NOT_AFTER + 1),
            Err(ChainError::Expired { key_id: line.release.id(), not_after: NOT_AFTER, now: NOT_AFTER + 1 })
        );
        assert_eq!(verify_manifest(&manifest, &envelope, &line.trust("prod"), NOT_AFTER), Ok(()));
        assert_eq!(
            verify_manifest(&manifest, &envelope, &line.trust("prod"), ISSUED_AT - 1),
            Err(ChainError::NotYetValid { key_id: line.release.id(), issued_at: ISSUED_AT, now: ISSUED_AT - 1 })
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
        assert_eq!(
            chain(&manifest, &envelope, &line),
            Err(ChainError::NoCertificateForSigner { key_id: other.id() })
        );
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
            Release { version: "1.200.0".into(), archive: Archive::TarGz, url: URL.into(), sha256: SHA.into(), size: 4096 }
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

    /// TEST0359: base64 is read strictly
    #[test]
    fn test0359_base64_is_read_strictly() {
        assert_eq!(base64_decode("TWFu").as_deref(), Some(&b"Man"[..]));
        assert_eq!(base64_decode("TWE=").as_deref(), Some(&b"Ma"[..]));
        assert_eq!(base64_decode("TQ==").as_deref(), Some(&b"M"[..]));
        for refused in ["", "TWF", "TW=u", "T===", "TWFu!AAA", "TR=="] {
            assert_eq!(base64_decode(refused), None, "{refused:?}");
        }
    }
}
