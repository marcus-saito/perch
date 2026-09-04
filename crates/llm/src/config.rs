//! Where the model lives, and whether the résumé is allowed to go there.
//!
//! A remote endpoint cannot be used for résumé import until the person has said
//! so for that feature. [`Model::may_send_document`] is the only place that
//! decides, and it refuses by default.

use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};
use std::net::IpAddr;
use std::path::Path;
use url::Url;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Model {
    /// An OpenAI-compatible base URL: Ollama, LM Studio, llama.cpp's server,
    /// OpenRouter, Fireworks. Empty means no model is configured, which is a
    /// supported way to run Perch.
    pub endpoint: String,
    pub model: String,
    pub consent: Consent,
}

/// Consent is per feature. Résumé import is the only feature listed, because it
/// is the only thing Perch would send anywhere.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Consent {
    /// Your résumé may be sent to a remote endpoint for import.
    pub resume_import_may_leave_this_mac: bool,
}

/// Why a document may or may not be sent to the configured endpoint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Permission {
    /// The endpoint is on this machine. Nothing leaves it.
    Local,
    /// Remote, and the person has said yes for this feature.
    ConsentedRemote { host: String },
    /// Remote, and they have not. Perch will not send.
    Refused { host: String },
    /// Nothing is configured, so there is nothing to send to.
    NoModel,
}

/// The host a request to this endpoint would actually reach.
///
/// Parsed with the same crate reqwest parses it with, so the host this answer
/// is about is the host the résumé would be sent to. Splitting the string by
/// hand instead lets the two disagree: a WHATWG parser ends the authority at a
/// backslash, so `http://evil.example\@localhost/v1` reads as loopback to a
/// hand-written check, and Perch would say "nothing leaves this machine" while
/// reqwest posted the document to evil.example.
fn host_of(endpoint: &str) -> Option<String> {
    let parsed = Url::parse(endpoint).ok()?;
    // An explicit http(s) scheme is required. Anything else is not a base URL
    // Perch could call, and guessing one would mean guessing where a résumé
    // goes.
    if !matches!(parsed.scheme(), "http" | "https") {
        return None;
    }
    parsed.host_str().map(str::to_ascii_lowercase)
}

/// Whether a request to this host is answered by this Mac. Anything else is
/// somebody else's computer, and a document sent there has left this machine.
///
/// Addresses are parsed rather than pattern-matched. A prefix test like
/// `starts_with("127.")` calls `127.0.0.1.evil.example` loopback, and a résumé
/// would go there without anyone being asked.
///
/// Reading an address on this Mac as remote is its own failure, and not a safe
/// one. It asks consent to send a résumé that never leaves, and it stores an
/// API key for a host that turns out to be a process on this machine, which
/// then receives that key as a bearer token.
fn is_loopback(host: &str) -> bool {
    // A URL parser brackets an IPv6 literal, and `[::1]` is not an address any
    // more than `::1` is a hostname. Unwrapping it here keeps someone running a
    // model on `[::1]` from being told their résumé leaves the machine when it
    // does not: a consent gate that cries wolf gets clicked through.
    let bare = host
        .strip_prefix('[')
        .and_then(|h| h.strip_suffix(']'))
        .unwrap_or(host);
    if let Ok(ip) = bare.parse::<IpAddr>() {
        // `::ffff:127.0.0.1` is 127.0.0.1 written the long way, and is not
        // loopback while it is still a v6 address. Unwrap it before asking.
        let ip = match ip {
            IpAddr::V6(v6) => v6.to_ipv4_mapped().map_or(ip, IpAddr::V4),
            v4 => v4,
        };
        // The unspecified address is answered by a socket on this machine, and
        // `0.0.0.0` is a common way to write "the server I am running here".
        return ip.is_loopback() || ip.is_unspecified();
    }
    // A trailing dot names the DNS root explicitly. `localhost.` and
    // `localhost` are the same host and resolve the same way.
    let name = bare.strip_suffix('.').unwrap_or(bare);
    is_a_reserved_localhost_name(name)
}

/// Whether this name is one RFC 6761 reserves for loopback: `localhost`, or
/// anything under `.localhost`.
///
/// The reservation covers the subtree whole. A registry may not grant a name
/// inside it, so nobody else can come to own one, and a resolver is expected to
/// answer it on loopback. macOS answers `foo.localhost` with 127.0.0.1 today.
///
/// This is a claim about the reservation, not about what a resolver did, and
/// that is the difference from `localhost.localdomain`. That name reads as this
/// machine while being ordinary: it sits in an ordinary namespace, nothing
/// stops it resolving anywhere, and macOS does not answer it at all. It is not
/// trusted here, and should not be added back.
///
/// What is left is a resolver that ignores the reservation and answers a
/// `.localhost` name with a real address. Resolving the name here would not
/// close that, because the address a name gives while the gate is deciding is
/// not the address the request gets afterwards.
fn is_a_reserved_localhost_name(name: &str) -> bool {
    if name == "localhost" {
        return true;
    }
    // An empty label is not a name anything answers, so `.localhost` on its own
    // and `..localhost` are not inside the subtree.
    match name.strip_suffix(".localhost") {
        Some(before) => !before.is_empty() && !before.ends_with('.'),
        None => false,
    }
}

impl Model {
    /// A missing file is not an error. No model configured is a supported way
    /// to run Perch, and every other feature works without one.
    pub fn load(path: &Path) -> Result<Self> {
        match std::fs::read_to_string(path) {
            Ok(text) => toml::from_str(&text).map_err(|source| Error::Config {
                path: path.display().to_string(),
                source: perch_core::TomlComplaint::new(&source, &text),
            }),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(err) => Err(err.into()),
        }
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        let text = toml::to_string_pretty(self).map_err(|e| Error::msg(e.to_string()))?;
        std::fs::write(path, text)?;
        Ok(())
    }

    pub fn configured(&self) -> bool {
        !self.endpoint.trim().is_empty() && !self.model.trim().is_empty()
    }

    pub fn host(&self) -> Option<String> {
        host_of(self.endpoint.trim())
    }

    pub fn is_local(&self) -> bool {
        self.host().as_deref().map(is_loopback).unwrap_or(false)
    }

    /// The only place that decides whether a document may be sent.
    pub fn may_send_document(&self) -> Permission {
        if !self.configured() {
            return Permission::NoModel;
        }
        let Some(host) = self.host() else {
            return Permission::NoModel;
        };
        if is_loopback(&host) {
            Permission::Local
        } else if self.consent.resume_import_may_leave_this_mac {
            Permission::ConsentedRemote { host }
        } else {
            Permission::Refused { host }
        }
    }

    /// The sentence the interface shows at the point of use.
    pub fn consequence(&self) -> String {
        match self.may_send_document() {
            Permission::NoModel => {
                "No model is configured. Watching, matching, filling and tracking need no model."
                    .to_string()
            }
            Permission::Local => format!(
                "{} runs on this Mac, so your résumé does not leave it.",
                self.model
            ),
            Permission::ConsentedRemote { host } => format!(
                "Your résumé is sent to {host} and leaves this Mac. You turned that on."
            ),
            Permission::Refused { host } => format!(
                "{host} is not on this Mac. Perch will not send your résumé there until you say it may."
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn remote() -> Model {
        Model {
            endpoint: "https://api.openrouter.ai/v1".into(),
            model: "qwen2.5:7b".into(),
            consent: Consent::default(),
        }
    }

    #[test]
    fn a_remote_endpoint_is_refused_until_it_is_allowed_for_this_feature() {
        let mut model = remote();
        assert!(matches!(
            model.may_send_document(),
            Permission::Refused { .. }
        ));
        assert!(model.consequence().contains("will not send"));

        model.consent.resume_import_may_leave_this_mac = true;
        match model.may_send_document() {
            Permission::ConsentedRemote { host } => assert_eq!(host, "api.openrouter.ai"),
            other => panic!("expected consented remote, got {other:?}"),
        }
        assert!(model.consequence().contains("leaves this Mac"));
    }

    #[test]
    fn loopback_needs_no_consent_because_nothing_leaves() {
        for endpoint in [
            "http://localhost:11434/v1",
            "http://127.0.0.1:1234/v1",
            "http://127.1.2.3:8080/v1",
            "http://[::1]:11434/v1",
            "http://localhost:8080",
        ] {
            let model = Model {
                endpoint: endpoint.into(),
                model: "llama3.1:8b".into(),
                consent: Consent::default(),
            };
            assert_eq!(
                model.may_send_document(),
                Permission::Local,
                "{endpoint} should be local"
            );
        }
    }

    #[test]
    fn the_localhost_subtree_is_this_machine_because_nobody_else_may_own_it() {
        // RFC 6761 reserves `.localhost` whole, so no registry hands one of
        // these to anyone, and macOS answers them on loopback. Someone running
        // a model at `ollama.localhost` is not sending their résumé anywhere,
        // and a consent gate that says otherwise gets clicked through.
        for endpoint in [
            "http://foo.localhost:11434/v1",
            "http://ollama.localhost/v1",
            "http://a.b.localhost:8080/v1",
            "http://foo.localhost.:11434/v1",
            "http://FOO.LOCALHOST:11434/v1",
        ] {
            let model = Model {
                endpoint: endpoint.into(),
                model: "m".into(),
                consent: Consent::default(),
            };
            assert_eq!(
                model.may_send_document(),
                Permission::Local,
                "{endpoint} should be local"
            );
        }
    }

    #[test]
    fn an_address_this_mac_answers_is_local_however_it_is_written() {
        // Every one of these reaches a socket on this machine. Reading one as
        // somebody else's computer is not a safe mistake: it asks consent to
        // send a résumé that never leaves, and it stores an API key for a host
        // that turns out to be a local process, which is then handed that key.
        for endpoint in [
            // The unspecified address, which is how people write "the server
            // I am running here".
            "http://0.0.0.0:11434/v1",
            "http://[::]:11434/v1",
            // A trailing dot names the DNS root and resolves the same way.
            "http://localhost.:11434/v1",
            "http://127.0.0.1.:11434/v1",
            // 127.0.0.1 written the long way.
            "http://[::ffff:127.0.0.1]:11434/v1",
        ] {
            let model = Model {
                endpoint: endpoint.into(),
                model: "m".into(),
                consent: Consent::default(),
            };
            assert_eq!(
                model.may_send_document(),
                Permission::Local,
                "{endpoint} should be local"
            );
            assert!(model.is_local(), "{endpoint} should want no key");
        }
    }

    #[test]
    fn a_config_file_is_never_quoted_back_when_it_fails_to_parse() {
        // A key pasted into model.toml must not come out again through the
        // error about it. The file is one a person is invited to open.
        let dir = std::env::temp_dir().join(format!("perch-model-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("a temp directory");
        let path = dir.join("model.toml");
        std::fs::write(
            &path,
            "endpoint = \"x\"\napi_key = \"sk-must-not-appear\"\n",
        )
        .expect("a written file");

        let failure = Model::load(&path).expect_err("an unknown field should not parse");
        let mut said = failure.to_string();
        let mut cause: &dyn std::error::Error = &failure;
        while let Some(next) = cause.source() {
            said.push_str(&next.to_string());
            cause = next;
        }
        std::fs::remove_dir_all(&dir).ok();

        assert!(!said.contains("sk-must-not-appear"), "{said}");
        assert!(said.contains("unknown field `api_key`"), "{said}");
    }

    #[test]
    fn a_host_that_merely_looks_local_is_still_somebody_elses_computer() {
        // The failure this guards against: a name that reads as loopback at a
        // glance and is not, sending a résumé off the machine silently.
        for endpoint in [
            "https://localhost.evil.example/v1",
            "https://127.0.0.1.evil.example/v1",
            "https://notlocalhost/v1",
            "http://192.168.1.9:11434/v1",
            "http://10.0.0.4/v1",
            "https://user@api.example.com/v1",
            // A backslash ends the authority for the parser reqwest uses, so
            // this reaches evil.example. Reading it as loopback would send the
            // résumé there under a promise that nothing left the machine.
            r"http://evil.example\@localhost/v1",
            r"http://evil.example\@127.0.0.1/v1",
            // A Linux hosts-file convention that macOS does not answer. It
            // reads as this machine and is whatever a resolver says it is.
            "http://localhost.localdomain:11434/v1",
            // The reserved subtree is a suffix, not a substring. Each of these
            // is an ordinary name that somebody else can own.
            "https://foo.localhost.evil.example/v1",
            "https://xlocalhost/v1",
            "https://my-localhost/v1",
            // An empty label is nothing a resolver answers, so it is not a way
            // into the subtree either.
            "http://.localhost/v1",
            "http://..localhost/v1",
        ] {
            let model = Model {
                endpoint: endpoint.into(),
                model: "m".into(),
                consent: Consent::default(),
            };
            assert!(
                matches!(model.may_send_document(), Permission::Refused { .. }),
                "{endpoint} was treated as local"
            );
        }
    }

    #[test]
    fn the_host_the_verdict_is_about_is_the_host_reqwest_would_reach() {
        // `may_send_document` is a promise about where a document goes. It can
        // only be kept if the host it reasons about is the host the request
        // actually resolves to, so the two parses are compared directly.
        for endpoint in [
            r"http://evil.example\@localhost/v1",
            "http://localhost:11434/v1",
            "https://someone:token@api.example.com/v1",
            "http://127.0.0.1/v1",
            r"http://localhost\@evil.example/v1",
        ] {
            let reqwest_would_reach = url::Url::parse(endpoint)
                .ok()
                .and_then(|u| u.host_str().map(str::to_string));
            assert_eq!(
                host_of(endpoint),
                reqwest_would_reach,
                "{endpoint} is judged against a different host than it reaches"
            );
        }
    }

    #[test]
    fn credentials_in_the_url_do_not_disguise_the_host() {
        assert_eq!(
            host_of("https://someone:token@api.example.com:8443/v1").as_deref(),
            Some("api.example.com")
        );
        assert_eq!(
            host_of("http://localhost@evil.example/v1").as_deref(),
            Some("evil.example")
        );
    }

    #[test]
    fn no_model_is_a_supported_state_not_an_error() {
        let model = Model::default();
        assert!(!model.configured());
        assert_eq!(model.may_send_document(), Permission::NoModel);
        assert!(model.consequence().contains("need no model"));

        let missing = Model::load(Path::new("/nowhere/at/all/model.toml")).unwrap();
        assert!(!missing.configured());
    }

    #[test]
    fn consent_is_named_for_the_one_feature_that_would_send_anything() {
        // If this ever becomes a general "allow remote" flag, the promise that
        // consent is per-feature has quietly been dropped.
        let text = toml::to_string_pretty(&Model {
            endpoint: "https://api.example.com/v1".into(),
            model: "m".into(),
            consent: Consent {
                resume_import_may_leave_this_mac: true,
            },
        })
        .unwrap();
        assert!(text.contains("resume_import_may_leave_this_mac"));

        // And an unknown consent key is refused rather than silently ignored.
        assert!(toml::from_str::<Model>("[consent]\nallow_everything = true").is_err());
    }

    #[test]
    fn an_endpoint_with_no_host_cannot_be_sent_to() {
        for endpoint in ["", "   ", "http://", "not a url", "://x"] {
            let model = Model {
                endpoint: endpoint.into(),
                model: "m".into(),
                consent: Consent {
                    resume_import_may_leave_this_mac: true,
                },
            };
            assert!(
                !matches!(
                    model.may_send_document(),
                    Permission::ConsentedRemote { .. }
                ),
                "{endpoint:?} produced a sendable endpoint"
            );
        }
    }
}
