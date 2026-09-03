//! The only thing in Perch that touches the network, and it only ever talks to
//! public job boards. Synchronous on purpose: one person, a handful of boards,
//! nothing worth a runtime.

use crate::error::{Error, Result};
use std::io::Read;
use std::time::Duration;

/// How much of a board's answer Perch is willing to hold in memory.
///
/// The largest real board measured is GitLab's, at 3.5 MB with every
/// description included, so this is roughly eighteen times the biggest thing
/// that legitimately arrives. It is a bound on a stranger's server, not a
/// guess at a size: without it a board that answers forever, or a small gzip
/// that expands into gigabytes, is only limited by the timeout.
const MOST_A_BOARD_MAY_SEND: u64 = 64 * 1024 * 1024;

/// The one place a response body is read, so every kind of answer is held to
/// the same bound.
///
/// Reads one byte past the limit so a body sitting exactly on it is not
/// mistaken for one that was cut short. The count is of decompressed bytes,
/// which is the number that has to be bounded.
fn read_capped(source: impl Read, url: &str) -> Result<Vec<u8>> {
    let mut body = Vec::new();
    source
        .take(MOST_A_BOARD_MAY_SEND + 1)
        .read_to_end(&mut body)?;
    if body.len() as u64 > MOST_A_BOARD_MAY_SEND {
        return Err(Error::msg(format!(
            "{url} sent more than {} MB, which is not a job board answering. Nothing was read.",
            MOST_A_BOARD_MAY_SEND / (1024 * 1024)
        )));
    }
    Ok(body)
}

pub struct Http {
    client: reqwest::blocking::Client,
}

impl Http {
    pub fn new() -> Result<Self> {
        let client = reqwest::blocking::Client::builder()
            .user_agent(concat!("perch/", env!("CARGO_PKG_VERSION")))
            .timeout(Duration::from_secs(20))
            .connect_timeout(Duration::from_secs(10))
            .build()?;
        Ok(Self { client })
    }

    /// `None` when the board is not there. A 404 is an answer, not a failure,
    /// and `watch add` needs to tell those apart.
    pub fn get_json(&self, url: &str) -> Result<Option<serde_json::Value>> {
        let Some(body) = self.get_bytes(url)? else {
            return Ok(None);
        };
        serde_json::from_slice(&body)
            .map(Some)
            .map_err(|e| Error::msg(format!("{url} did not answer with JSON: {e}")))
    }

    /// A page as text, with `None` meaning not there, the same as `get_json`.
    ///
    /// Bytes that are not UTF-8 are replaced rather than refused. This is
    /// display text, and one bad byte in a careers page is not a reason to
    /// lose the postings around it.
    ///
    /// A page that is already UTF-8, which every real one measured was, is
    /// taken as it stands. Copying it to replace nothing would hold a body
    /// arriving at the cap twice while the copy was made.
    pub fn get_html(&self, url: &str) -> Result<Option<String>> {
        Ok(self
            .get_bytes(url)?
            .map(|body| match String::from_utf8(body) {
                Ok(text) => text,
                Err(not_utf8) => String::from_utf8_lossy(not_utf8.as_bytes()).into_owned(),
            }))
    }

    /// Ask, then read what came back. `None` for a 404, for the reason given
    /// on `get_json`.
    fn get_bytes(&self, url: &str) -> Result<Option<Vec<u8>>> {
        let response = self.client.get(url).send()?;
        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }
        let response = response.error_for_status()?;
        read_capped(response, url).map(Some)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::net::TcpListener;
    use std::thread::{self, JoinHandle};

    /// One canned answer on loopback, so reading a response can be tested
    /// without asking a real board. The handle yields the request that
    /// arrived, for the tests that care what Perch said.
    fn serve(status: &str, body: &[u8]) -> (String, JoinHandle<String>) {
        let mut response = format!(
            "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        )
        .into_bytes();
        response.extend_from_slice(body);

        let listener = TcpListener::bind("127.0.0.1:0").expect("a loopback port");
        let port = listener.local_addr().expect("an address").port();
        let handle = thread::spawn(move || {
            let (mut socket, _) = listener.accept().expect("a connection");
            // A GET carries no body, so its headers arrive in one read.
            let mut request = [0u8; 2048];
            let read = socket.read(&mut request).unwrap_or(0);
            socket.write_all(&response).ok();
            socket.flush().ok();
            String::from_utf8_lossy(&request[..read]).into_owned()
        });
        (format!("http://127.0.0.1:{port}/careers"), handle)
    }

    #[test]
    fn a_page_comes_back_as_text() {
        let (url, server) = serve("200 OK", b"<html><p>A role.</p></html>");
        let page = Http::new()
            .expect("a client")
            .get_html(&url)
            .expect("a page");
        assert_eq!(page.as_deref(), Some("<html><p>A role.</p></html>"));
        server.join().expect("the server thread");
    }

    #[test]
    fn a_page_that_is_not_there_comes_back_as_none() {
        let (url, server) = serve("404 Not Found", b"");
        let page = Http::new()
            .expect("a client")
            .get_html(&url)
            .expect("an answer");
        assert_eq!(page, None);
        server.join().expect("the server thread");
    }

    #[test]
    fn a_page_with_a_byte_that_is_not_utf8_is_still_read() {
        // Latin-1 in a careers page. The bad byte becomes a replacement
        // character and the words around it survive.
        let (url, server) = serve("200 OK", b"<p>caf\xe9 role</p>");
        let page = Http::new()
            .expect("a client")
            .get_html(&url)
            .expect("a page");
        assert_eq!(page.as_deref(), Some("<p>caf\u{fffd} role</p>"));
        server.join().expect("the server thread");
    }

    #[test]
    fn a_page_the_server_refused_is_an_error_not_an_empty_page() {
        let (url, server) = serve("500 Internal Server Error", b"try later");
        assert!(Http::new().expect("a client").get_html(&url).is_err());
        server.join().expect("the server thread");
    }

    #[test]
    fn a_board_that_answers_with_json_still_parses() {
        let (url, server) = serve("200 OK", br#"{"jobs":[{"id":7}]}"#);
        let body = Http::new()
            .expect("a client")
            .get_json(&url)
            .expect("a board")
            .expect("a body");
        assert_eq!(body["jobs"][0]["id"], 7);
        server.join().expect("the server thread");
    }

    #[test]
    fn every_request_says_it_is_perch() {
        let (url, server) = serve("200 OK", b"<p>A role.</p>");
        Http::new()
            .expect("a client")
            .get_html(&url)
            .expect("a page");
        let request = server.join().expect("the server thread").to_lowercase();
        let honest = format!("user-agent: perch/{}", env!("CARGO_PKG_VERSION"));
        assert!(
            request.contains(&honest),
            "asked as something else: {request}"
        );
    }

    #[test]
    fn a_body_sitting_exactly_on_the_cap_is_kept() {
        let body = read_capped(
            std::io::repeat(b'x').take(MOST_A_BOARD_MAY_SEND),
            "https://example.test/careers",
        )
        .expect("a body at the cap");
        assert_eq!(body.len() as u64, MOST_A_BOARD_MAY_SEND);
    }

    #[test]
    fn a_body_one_byte_over_the_cap_is_refused() {
        let refused = read_capped(
            std::io::repeat(b'x').take(MOST_A_BOARD_MAY_SEND + 1),
            "https://example.test/careers",
        )
        .expect_err("a body over the cap");
        let said = refused.to_string();
        assert!(said.contains("https://example.test/careers"), "{said}");
        assert!(said.contains("64 MB"), "{said}");
    }

    #[test]
    fn a_body_that_never_ends_stops_at_the_cap() {
        // The shape the cap exists for. The read returns instead of growing
        // until the process runs out of memory.
        let refused = read_capped(std::io::repeat(b'x'), "https://example.test/careers");
        assert!(refused.is_err());
    }
}
