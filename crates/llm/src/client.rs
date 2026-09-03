//! One OpenAI-compatible client, covering Ollama, LM Studio, llama.cpp's
//! server, OpenRouter and Fireworks.
//!
//! No inference happens in this process. Perch sends a request to whatever the
//! person pointed it at and reads the reply. The reply is a proposal, and it
//! has to pass [`crate::verify`] before anyone sees it.

use crate::config::{Model, Permission};
use crate::error::{Error, Result};
use serde::Deserialize;
use serde_json::{json, Value};
use std::time::Duration;

/// Ollama's default port. Probed at launch so a model already installed on
/// this Mac needs no configuration.
pub const OLLAMA: &str = "http://localhost:11434";

pub struct Client {
    http: reqwest::blocking::Client,
}

/// A model the endpoint says it can run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Available {
    pub name: String,
    /// Ollama reports a size; other endpoints do not.
    pub size: Option<u64>,
}

impl Client {
    pub fn new() -> Result<Self> {
        Ok(Self {
            http: reqwest::blocking::Client::builder()
                .user_agent(concat!("perch/", env!("CARGO_PKG_VERSION")))
                // The gate judges one host. A redirect would re-post the whole
                // request, résumé and all, to whatever host its Location names,
                // so this client does not follow one.
                .redirect(reqwest::redirect::Policy::none())
                // A proxy variable in the environment would do the same thing
                // by another route, including when the endpoint is on this Mac
                // and Perch has said nothing leaves it.
                .no_proxy()
                // A small model on a laptop can take a while to answer. The
                // person is waiting on this deliberately.
                .timeout(Duration::from_secs(180))
                .connect_timeout(Duration::from_secs(5))
                .build()?,
        })
    }

    /// Is Ollama running on this Mac, and what has it got? `None` means it is
    /// not there, which is not a failure.
    pub fn probe_ollama(&self) -> Option<Vec<Available>> {
        let response = self
            .http
            .get(format!("{OLLAMA}/api/tags"))
            .timeout(Duration::from_secs(2))
            .send()
            .ok()?;
        let body: Value = response.json().ok()?;
        let models = body.get("models")?.as_array()?;
        Some(
            models
                .iter()
                .filter_map(|m| {
                    Some(Available {
                        name: m.get("name")?.as_str()?.to_string(),
                        size: m.get("size").and_then(Value::as_u64),
                    })
                })
                .collect(),
        )
    }

    /// What an OpenAI-compatible endpoint says it can run.
    pub fn list_models(&self, endpoint: &str, key: Option<&str>) -> Result<Vec<Available>> {
        let url = format!("{}/models", endpoint.trim_end_matches('/'));
        let mut request = self.http.get(url);
        if let Some(key) = key {
            request = request.bearer_auth(key);
        }
        let body: Value = request.send()?.error_for_status()?.json()?;
        let data = body
            .get("data")
            .and_then(Value::as_array)
            .ok_or(Error::BadShape)?;
        Ok(data
            .iter()
            .filter_map(|m| {
                Some(Available {
                    name: m.get("id")?.as_str()?.to_string(),
                    size: None,
                })
            })
            .collect())
    }

    /// Ask for one JSON answer in a given shape.
    ///
    /// Endpoints disagree about how to constrain output, so this walks down:
    /// a JSON schema first, then plain JSON mode, then nothing. That is a
    /// convenience, not a safety measure. An endpoint that ignores all three
    /// still cannot get an unanchored value in front of anyone, because
    /// verification happens afterwards.
    pub fn ask_for_json(
        &self,
        model: &Model,
        key: Option<&str>,
        system: &str,
        user: &str,
        schema: Value,
    ) -> Result<Value> {
        // The gate. A document only leaves this machine with permission.
        match model.may_send_document() {
            Permission::NoModel => return Err(Error::NoModel),
            Permission::Refused { host } => return Err(Error::ConsentMissing(host)),
            Permission::Local | Permission::ConsentedRemote { .. } => {}
        }

        let url = format!(
            "{}/chat/completions",
            model.endpoint.trim().trim_end_matches('/')
        );
        let messages = json!([
            { "role": "system", "content": system },
            { "role": "user", "content": user },
        ]);

        let shapes = [
            Some(json!({
                "type": "json_schema",
                "json_schema": { "name": "extraction", "strict": true, "schema": schema },
            })),
            Some(json!({ "type": "json_object" })),
            None,
        ];

        let mut last: Option<Error> = None;
        for shape in shapes {
            let mut body = json!({
                "model": model.model,
                "messages": messages,
                "temperature": 0,
                "stream": false,
            });
            if let Some(shape) = shape {
                body["response_format"] = shape;
            }

            let mut request = self.http.post(&url).json(&body);
            if let Some(key) = key {
                request = request.bearer_auth(key);
            }
            match request.send().and_then(|r| r.error_for_status()) {
                Ok(response) if response.status().is_redirection() => {
                    // Not followed, and said out loud rather than left to fail
                    // as a reply that would not parse. The host a redirect
                    // names is not the host anyone was asked about.
                    return Err(Error::Redirected(model.endpoint.trim().to_string()));
                }
                Ok(response) => {
                    let reply: Reply = response.json()?;
                    let content = reply
                        .choices
                        .first()
                        .map(|c| c.message.content.as_str())
                        .unwrap_or_default();
                    return parse_json_body(content).ok_or(Error::BadShape);
                }
                Err(err) => {
                    let status = err.status().map(|s| s.as_u16());
                    // Turned away for want of a key. Said in those terms
                    // because reqwest's own words name a status and a URL, and
                    // the thing to do about it is neither of those.
                    if status == Some(401) || status == Some(403) {
                        return Err(Error::Unauthorized {
                            host: model
                                .host()
                                .unwrap_or_else(|| model.endpoint.trim().to_string()),
                        });
                    }
                    // A rejected response_format is worth retrying without it;
                    // anything else is a real failure and should be reported.
                    let retryable = status.map(|s| s == 400 || s == 422);
                    last = Some(Error::Http(err));
                    if retryable != Some(true) {
                        break;
                    }
                }
            }
        }
        Err(last.unwrap_or(Error::BadShape))
    }
}

#[derive(Deserialize)]
struct Reply {
    choices: Vec<Choice>,
}

#[derive(Deserialize)]
struct Choice {
    message: Message,
}

#[derive(Deserialize)]
struct Message {
    #[serde(default)]
    content: String,
}

/// Small models wrap JSON in prose or a fenced block often enough that it is
/// worth digging it out rather than failing the import.
pub fn parse_json_body(content: &str) -> Option<Value> {
    let text = content.trim();
    if let Ok(value) = serde_json::from_str::<Value>(text) {
        return Some(value);
    }
    // A fenced block, with or without a language tag.
    if let Some(rest) = text.split("```").nth(1) {
        let body = rest.strip_prefix("json").unwrap_or(rest);
        if let Ok(value) = serde_json::from_str::<Value>(body.trim()) {
            return Some(value);
        }
    }
    // The first balanced object in the reply.
    let start = text.find('{')?;
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;
    for (at, ch) in text[start..].char_indices() {
        if in_string {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }
        match ch {
            '"' => in_string = true,
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return serde_json::from_str(&text[start..start + at + 1]).ok();
                }
            }
            _ => {}
        }
    }
    None
}

/// The API key for a remote endpoint lives in the system keychain, never in a
/// file Perch writes. There is nowhere in `model.toml` to put one.
pub mod secret {
    const SERVICE: &str = "dev.perch.app";

    pub fn get(host: &str) -> Option<String> {
        keyring::Entry::new(SERVICE, host).ok()?.get_password().ok()
    }

    pub fn set(host: &str, key: &str) -> Result<(), String> {
        keyring::Entry::new(SERVICE, host)
            .and_then(|e| e.set_password(key))
            .map_err(|e| e.to_string())
    }

    pub fn forget(host: &str) -> Result<(), String> {
        match keyring::Entry::new(SERVICE, host).map(|e| e.delete_credential()) {
            Ok(Ok(())) | Err(keyring::Error::NoEntry) => Ok(()),
            Ok(Err(keyring::Error::NoEntry)) => Ok(()),
            Ok(Err(e)) | Err(e) => Err(e.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Consent;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread::{self, JoinHandle};

    /// Stands in for the résumé, so a test can say whether the document itself
    /// reached somewhere it was never meant to.
    const DOCUMENT: &str = "Dana Ferreira, 14 Marrow Lane, Portland OR";

    fn at(port: u16) -> Model {
        Model {
            endpoint: format!("http://127.0.0.1:{port}/v1"),
            model: "m".into(),
            consent: Consent::default(),
        }
    }

    /// A loopback port with a thread waiting on it, which answers whatever
    /// arrives and hands back what it was sent. It stands where a document
    /// must not go, so what a test reads off it is the whole request.
    fn watcher() -> (u16, JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a loopback port");
        let port = listener.local_addr().expect("an address").port();
        let handle = thread::spawn(move || {
            let (mut socket, _) = listener.accept().expect("a connection");
            let mut sent = [0u8; 8192];
            let read = socket.read(&mut sent).unwrap_or(0);
            socket
                .write_all(b"HTTP/1.1 502 Bad Gateway\r\nConnection: close\r\n\r\n")
                .ok();
            String::from_utf8_lossy(&sent[..read]).into_owned()
        });
        (port, handle)
    }

    /// Let a watcher finish when nothing was sent to it, which is the passing
    /// case and would otherwise be a thread waiting forever.
    fn knock(port: u16) {
        std::net::TcpStream::connect(("127.0.0.1", port)).ok();
    }

    /// A loopback port with nothing listening on it, so only a proxy could
    /// answer a request sent there.
    fn nobody_listening() -> u16 {
        TcpListener::bind("127.0.0.1:0")
            .expect("a loopback port")
            .local_addr()
            .expect("an address")
            .port()
    }

    fn local() -> Model {
        Model {
            endpoint: "http://localhost:11434/v1".into(),
            model: "llama3.1:8b".into(),
            consent: Consent::default(),
        }
    }

    #[test]
    fn a_remote_endpoint_without_consent_never_reaches_the_network() {
        let client = Client::new().unwrap();
        let model = Model {
            endpoint: "https://api.example.com/v1".into(),
            model: "m".into(),
            consent: Consent::default(),
        };
        // No request is attempted. The refusal happens before any I/O, so this
        // passes with the network unavailable.
        let err = client
            .ask_for_json(&model, None, "s", "u", json!({}))
            .unwrap_err();
        assert!(matches!(err, Error::ConsentMissing(host) if host == "api.example.com"));
    }

    #[test]
    fn with_no_model_configured_there_is_nothing_to_ask() {
        let client = Client::new().unwrap();
        let err = client
            .ask_for_json(&Model::default(), None, "s", "u", json!({}))
            .unwrap_err();
        assert!(matches!(err, Error::NoModel));
    }

    #[test]
    fn json_is_dug_out_of_whatever_a_small_model_wraps_it_in() {
        let wanted = json!({ "name": "Dana Ferreira", "skills": ["Rust"] });
        for reply in [
            r#"{"name":"Dana Ferreira","skills":["Rust"]}"#,
            "```json\n{\"name\":\"Dana Ferreira\",\"skills\":[\"Rust\"]}\n```",
            "```\n{\"name\":\"Dana Ferreira\",\"skills\":[\"Rust\"]}\n```",
            "Here is the JSON you asked for:\n{\"name\":\"Dana Ferreira\",\"skills\":[\"Rust\"]}\nHope that helps",
        ] {
            assert_eq!(parse_json_body(reply).as_ref(), Some(&wanted), "{reply:?}");
        }
    }

    #[test]
    fn braces_inside_strings_do_not_end_the_object() {
        let value =
            parse_json_body(r#"prose {"note":"a } inside \" a string","n":1} tail"#).unwrap();
        assert_eq!(value["n"], 1);
        assert_eq!(value["note"], "a } inside \" a string");
    }

    #[test]
    fn a_reply_with_no_json_in_it_is_not_guessed_at() {
        for reply in [
            "",
            "I cannot help with that.",
            "{unbalanced",
            "```json\n{oops\n```",
        ] {
            assert!(parse_json_body(reply).is_none(), "{reply:?}");
        }
    }

    #[test]
    fn an_endpoint_that_wants_a_key_says_so_rather_than_naming_a_status() {
        // The remedy for a 401 is a key, and neither the status code nor the
        // URL says that. A person reading "HTTP status client error (401
        // Unauthorized)" has to know what Perch was doing to make sense of it.
        let listener = TcpListener::bind("127.0.0.1:0").expect("a loopback port");
        let port = listener.local_addr().expect("an address").port();
        let server = thread::spawn(move || {
            let (mut socket, _) = listener.accept().expect("a connection");
            let mut request = [0u8; 8192];
            // The request itself does not matter here, only that one arrived.
            let _ = socket.read(&mut request).unwrap_or(0);
            socket
                .write_all(
                    b"HTTP/1.1 401 Unauthorized
Content-Length: 0
Connection: close

",
                )
                .ok();
        });

        let err = Client::new()
            .expect("a client")
            .ask_for_json(&at(port), None, "s", DOCUMENT, json!({}))
            .unwrap_err();
        server.join().expect("the server thread");

        match err {
            Error::Unauthorized { host } => assert_eq!(host, "127.0.0.1"),
            other => panic!("a refused key came back as {other:?}"),
        }
    }

    #[test]
    fn a_redirect_does_not_carry_the_document_to_another_host() {
        // Two loopback ports: the endpoint the gate judged, and the host its
        // answer points at. Only the first may see the document.
        let (named, elsewhere) = watcher();
        let endpoint = TcpListener::bind("127.0.0.1:0").expect("a loopback port");
        let port = endpoint.local_addr().expect("an address").port();
        let server = thread::spawn(move || {
            let (mut socket, _) = endpoint.accept().expect("a connection");
            let mut request = [0u8; 8192];
            let read = socket.read(&mut request).unwrap_or(0);
            assert!(read > 0, "the endpoint was asked for something");
            let answer = format!(
                "HTTP/1.1 307 Temporary Redirect\r\n\
                 Location: http://127.0.0.1:{named}/v1/chat/completions\r\n\
                 Content-Length: 0\r\nConnection: close\r\n\r\n"
            );
            socket.write_all(answer.as_bytes()).ok();
            socket.flush().ok();
        });

        let err = Client::new()
            .expect("a client")
            .ask_for_json(&at(port), None, "s", DOCUMENT, json!({}))
            .unwrap_err();
        server.join().expect("the server thread");
        knock(named);
        let sent = elsewhere.join().expect("the watcher thread");

        assert!(
            !sent.contains(DOCUMENT),
            "the document was carried to the host the redirect named"
        );
        assert!(matches!(err, Error::Redirected(_)), "{err:?}");
    }

    #[test]
    fn a_proxy_in_the_environment_does_not_receive_the_document() {
        // A shell commonly carries these. The endpoint here is on this Mac,
        // where Perch says nothing leaves it, and nothing is listening on it,
        // so a proxy is the only thing that could be handed the document.
        let (port, proxy) = watcher();
        std::env::set_var("ALL_PROXY", format!("http://127.0.0.1:{port}"));
        std::env::set_var("HTTP_PROXY", format!("http://127.0.0.1:{port}"));
        let client = Client::new().expect("a client");
        std::env::remove_var("ALL_PROXY");
        std::env::remove_var("HTTP_PROXY");

        let err = client
            .ask_for_json(&at(nobody_listening()), None, "s", DOCUMENT, json!({}))
            .unwrap_err();
        knock(port);
        let sent = proxy.join().expect("the watcher thread");

        assert!(!sent.contains(DOCUMENT), "the document was sent to a proxy");
        assert!(matches!(err, Error::Http(_)), "{err:?}");
    }

    #[test]
    fn a_local_endpoint_needs_no_consent_to_be_asked() {
        // It will fail to connect here. What matters is that it got past the
        // gate rather than being refused.
        let client = Client::new().unwrap();
        let err = client
            .ask_for_json(&local(), None, "s", "u", json!({}))
            .unwrap_err();
        assert!(
            !matches!(err, Error::ConsentMissing(_) | Error::NoModel),
            "a local endpoint was gated: {err:?}"
        );
    }
}
