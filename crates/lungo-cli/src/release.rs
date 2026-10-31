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
/// A registry's charter and its signature, under the registry's anchor.
const CHARTER_FILE: &str = "charter.json";
const CHARTER_SIG_FILE: &str = "charter.json.sig";
const CHARTER_FORMAT: &str = "machinefabric-registry-charter/1";
const CHARTER_ENVELOPE_FORMAT: &str = "machinefabric-charter-sig/1";
/// The label a charter's certificate carries: charters are signed by prod for every environment.
const CHARTER_SIGNING_ENVIRONMENT: &str = "prod";
const MANIFEST_SCHEMA_MAJOR: &str = "5";
/// Distinct roots that must have signed a certificate.
const REQUIRED_ROOT_SIGNATURES: usize = 2;

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
                write!(
                    f,
                    "the manifest is signed by key {key_id}, which no certificate in the signature file authorizes"
                )
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
    manifest_signature: DocumentSignature,
}

/// A charter's signature file: the manifests' chain, with a format word and a signature field of
/// its own, so that no signed manifest can be passed off as a charter.
#[derive(Deserialize)]
struct CharterEnvelope {
    certificates: Vec<CertificateEntry>,
    charter_signature: DocumentSignature,
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
struct DocumentSignature {
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
    authorize(&envelope.certificates, &envelope.manifest_signature, manifest, trust, now)
}

/// The chain under every signed document: every certificate valid under `trust`, and `signature`
/// over the exact `bytes` by a release key one of them certifies.
fn authorize(
    certificates: &[CertificateEntry],
    signature: &DocumentSignature,
    bytes: &[u8],
    trust: &Trust,
    now: u64,
) -> std::result::Result<(), ChainError> {
    if certificates.is_empty() {
        return Err(ChainError::NoCertificates);
    }
    let mut release_keys: Vec<(String, String)> = Vec::new();
    for entry in certificates {
        let certificate = verify_certificate(entry, trust, now)?;
        release_keys.push((certificate.key_id, certificate.release_pubkey));
    }
    let signer = &signature.key_id;
    let Some((_, release_key)) = release_keys.iter().find(|(id, _)| id == signer) else {
        return Err(ChainError::NoCertificateForSigner { key_id: signer.clone() });
    };
    verify_signature(release_key, &signature.signature, bytes).map_err(|e| match e {
        SignatureError::Malformed(why) => ChainError::MalformedKey(why),
        SignatureError::Mismatch(why) => ChainError::ManifestSignatureInvalid(why),
    })
}

/// Why a registry's charter is not one this `lungo` can read its releases through.
#[derive(Debug, PartialEq, Eq)]
pub enum CharterError {
    /// The signature file is not a charter's.
    MalformedEnvelope(String),
    UnsupportedEnvelopeFormat(String),
    /// The certificates do not chain to MachineFabric's roots under the `prod` label, or the
    /// charter is not signed by a key they certify.
    Chain(ChainError),
    /// The charter's signature does not match its bytes.
    SignatureInvalid(String),
    Malformed(String),
    UnsupportedFormat(String),
    AnchorMismatch {
        charter: String,
        asked: String,
    },
    KindMismatch(String),
    EnvironmentMismatch {
        charter: String,
        build: String,
    },
    NotYetValid {
        issued_at: u64,
        now: u64,
    },
    Expired {
        not_after: u64,
        now: u64,
    },
    /// The charter's manifest location is not one the command's manifest address can be read
    /// from.
    Location(String),
}

impl fmt::Display for CharterError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CharterError::MalformedEnvelope(e) => write!(f, "its signature file is not a charter signature: {e}"),
            CharterError::UnsupportedEnvelopeFormat(found) => {
                write!(f, "its signature file is of format {found:?}, not {CHARTER_ENVELOPE_FORMAT:?}")
            }
            CharterError::Chain(ChainError::WrongEnvironment { key_id, certificate, .. }) => write!(
                f,
                "it is signed by release key {key_id}, certified for {certificate}; a charter is signed by \
                 MachineFabric's {CHARTER_SIGNING_ENVIRONMENT} release key"
            ),
            CharterError::Chain(e) => write!(f, "it does not chain to MachineFabric's roots: {e}"),
            CharterError::SignatureInvalid(e) => write!(f, "its signature does not match it: {e}"),
            CharterError::Malformed(e) => write!(f, "it is not a charter: {e}"),
            CharterError::UnsupportedFormat(found) => write!(f, "it is of format {found:?}, not {CHARTER_FORMAT:?}"),
            CharterError::AnchorMismatch { charter, asked } => {
                write!(f, "it is the charter of {charter}, not of {asked}")
            }
            CharterError::KindMismatch(kind) => write!(f, "it charters a {kind} registry, not a release registry"),
            CharterError::EnvironmentMismatch { charter, build } => {
                write!(f, "its registry is signed under {charter}; this lungo was built for {build}")
            }
            CharterError::NotYetValid { issued_at, now } => write!(f, "it is valid from {issued_at} (it is {now})"),
            CharterError::Expired { not_after, now } => write!(f, "it expired at {not_after} (it is {now})"),
            CharterError::Location(e) => f.write_str(e),
        }
    }
}

/// A charter body, as signed: every field a charter has, and no other.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
#[allow(dead_code)]
struct Charter {
    format: String,
    anchor: String,
    kind: String,
    slug: String,
    environment: String,
    #[serde(default)]
    experiment: Option<String>,
    locations: BTreeMap<String, String>,
    #[serde(default)]
    fabric: Option<String>,
    issued_at: u64,
    not_after: Option<u64>,
}

/// Checks that `envelope` (the bytes of `charter.json.sig`) authorizes `charter` (the exact bytes
/// of `charter.json`) under MachineFabric's `charter_roots`, and that it charters the release
/// registry at `anchor` for the `environment` this `lungo` verifies, at the time `now`. Returns
/// the address of the command's manifest, as the charter states it.
pub fn verify_charter(
    charter: &[u8],
    envelope: &[u8],
    charter_roots: &[String],
    anchor: &str,
    environment: &str,
    now: u64,
) -> std::result::Result<String, CharterError> {
    let probe: serde_json::Value =
        serde_json::from_slice(envelope).map_err(|e| CharterError::MalformedEnvelope(e.to_string()))?;
    match probe.get("format").and_then(|f| f.as_str()) {
        Some(CHARTER_ENVELOPE_FORMAT) => {}
        Some(other) => return Err(CharterError::UnsupportedEnvelopeFormat(other.to_owned())),
        None => return Err(CharterError::MalformedEnvelope("it has no format".to_owned())),
    }
    let envelope: CharterEnvelope =
        serde_json::from_value(probe).map_err(|e| CharterError::MalformedEnvelope(e.to_string()))?;
    let trust = Trust { roots: charter_roots.to_vec(), environment: CHARTER_SIGNING_ENVIRONMENT.to_owned() };
    authorize(&envelope.certificates, &envelope.charter_signature, charter, &trust, now).map_err(|e| match e {
        ChainError::ManifestSignatureInvalid(why) => CharterError::SignatureInvalid(why),
        other => CharterError::Chain(other),
    })?;

    // Only now is the body read: nothing about an unsigned document is believed, including what
    // it says its format is.
    let body: serde_json::Value =
        serde_json::from_slice(charter).map_err(|e| CharterError::Malformed(e.to_string()))?;
    match body.get("format").and_then(|f| f.as_str()) {
        Some(CHARTER_FORMAT) => {}
        Some(other) => return Err(CharterError::UnsupportedFormat(other.to_owned())),
        None => return Err(CharterError::Malformed("it has no format".to_owned())),
    }
    let body: Charter = serde_json::from_value(body).map_err(|e| CharterError::Malformed(e.to_string()))?;
    if body.anchor != anchor {
        return Err(CharterError::AnchorMismatch { charter: body.anchor, asked: anchor.to_owned() });
    }
    if body.kind != "releases" {
        return Err(CharterError::KindMismatch(body.kind));
    }
    if body.environment != environment {
        return Err(CharterError::EnvironmentMismatch { charter: body.environment, build: environment.to_owned() });
    }
    if body.issued_at > now {
        return Err(CharterError::NotYetValid { issued_at: body.issued_at, now });
    }
    if let Some(not_after) = body.not_after
        && now > not_after
    {
        return Err(CharterError::Expired { not_after, now });
    }
    manifest_location(&body.locations)
}

/// The command's manifest address from a release registry's `manifest` location: a template that
/// takes `{app_id}` and nothing else.
fn manifest_location(locations: &BTreeMap<String, String>) -> std::result::Result<String, CharterError> {
    let Some(template) = locations.get("manifest") else {
        return Err(CharterError::Location("the charter states no manifest location".to_owned()));
    };
    let filled = template.replace("{app_id}", APP_ID);
    if filled == *template || filled.contains(['{', '}']) {
        return Err(CharterError::Location(format!(
            "the charter's manifest location {template:?} does not take exactly {{app_id}}"
        )));
    }
    Ok(filled)
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
    let key =
        ed25519_dalek::VerifyingKey::from_bytes(&key.ed25519).map_err(|e| SignatureError::Malformed(e.to_string()))?;
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
    let charter_url = format!("{registry}/{CHARTER_FILE}");
    let Some(charter) = source.get(&charter_url)? else {
        return Err(Error::UpdateUntrusted(format!(
            "{registry} has no charter ({charter_url} is missing), so it is not a release registry MachineFabric chartered"
        )));
    };
    let charter_sig_url = format!("{registry}/{CHARTER_SIG_FILE}");
    let Some(charter_sig) = source.get(&charter_sig_url)? else {
        return Err(Error::UpdateUntrusted(format!(
            "{registry} has a charter and no signature for it ({charter_sig_url} is missing)"
        )));
    };
    let url = verify_charter(&charter, &charter_sig, charter_roots, registry, &trust.environment, now)
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

    fn chain(manifest: &[u8], envelope: &[u8], line: &Line) -> std::result::Result<(), ChainError> {
        verify_manifest(manifest, envelope, &line.trust("prod"), NOW)
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
            verify_charter(charter, &line.sign_charter(charter), &roots, anchor, environment, now)
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
        let cartridges = charter_at(&|b| b["kind"] = serde_json::json!("cartridges"));
        assert_eq!(verify(&cartridges, "prod", NOW), Err(CharterError::KindMismatch("cartridges".into())));
        assert_eq!(
            verify(&charter, "staging", NOW),
            Err(CharterError::EnvironmentMismatch { charter: "prod".into(), build: "staging".into() })
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
        assert!(matches!(verify(&no_app, "prod", NOW), Err(CharterError::Location(_))));
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
            verify_charter(body, envelope, roots, anchor, "staging", NOW)
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
            Err(CharterError::Chain(ChainError::WrongEnvironment { .. }))
        ));
        // Roots other than the charter roots.
        let strangers = Line { roots: [Key::new(21), Key::new(22), Key::new(23)], release: Key::new(29) };
        assert!(matches!(
            verify(&line.sign_charter(&charter), &charter, &strangers.root_keys()),
            Err(CharterError::Chain(ChainError::TooFewRootSignatures { .. }))
        ));
        // A prod-signed manifest signature over the same bytes is not a charter's.
        assert_eq!(
            verify(&line.sign(&charter), &charter, &roots),
            Err(CharterError::UnsupportedEnvelopeFormat(ENVELOPE_FORMAT.into()))
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
        assert_eq!(chain(&manifest, &envelope, &line), Err(ChainError::NoCertificateForSigner { key_id: other.id() }));
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
