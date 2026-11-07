//! `ReqwestClient` redirect handling, against a real HTTP server on 127.0.0.1.

#![cfg(feature = "reqwest")]

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};
use std::thread;

use shoutrrr::transport::{HttpClient, HttpRequest, RedirectPolicy, ReqwestClient};

/// A minimal HTTP/1.1 server. `/hook` answers `302` to `/landed`, `/no-location` answers `302`
/// without a `Location`, and anything else answers `200`. It records each request as
/// `"METHOD /path"`.
struct Server {
    base: String,
    seen: Arc<Mutex<Vec<String>>>,
}

impl Server {
    fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("127.0.0.1:{}", listener.local_addr().unwrap().port());
        let seen = Arc::new(Mutex::new(Vec::new()));
        let log = Arc::clone(&seen);
        thread::spawn(move || {
            for stream in listener.incoming() {
                let mut stream = stream.unwrap();
                let mut reader = BufReader::new(stream.try_clone().unwrap());

                let mut request_line = String::new();
                reader.read_line(&mut request_line).unwrap();
                let mut parts = request_line.split_whitespace();
                let (method, path) = (parts.next().unwrap(), parts.next().unwrap());

                let mut content_length = 0;
                loop {
                    let mut line = String::new();
                    reader.read_line(&mut line).unwrap();
                    let line = line.trim_end();
                    if line.is_empty() {
                        break;
                    }
                    if let Some((name, value)) = line.split_once(':')
                        && name.eq_ignore_ascii_case("content-length")
                    {
                        content_length = value.trim().parse().unwrap();
                    }
                }
                reader.read_exact(&mut vec![0; content_length]).unwrap();
                log.lock().unwrap().push(format!("{method} {path}"));

                let head = match path {
                    "/hook" => "302 Found\r\nLocation: /landed",
                    "/no-location" => "302 Found",
                    _ => "200 OK",
                };
                write!(
                    stream,
                    "HTTP/1.1 {head}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                )
                .unwrap();
            }
        });
        Self { base, seen }
    }

    fn url(&self, path: &str) -> String {
        format!("http://{}{path}", self.base)
    }

    fn seen(&self) -> Vec<String> {
        self.seen.lock().unwrap().clone()
    }
}

fn block_on<F: std::future::Future>(future: F) -> F::Output {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(future)
}

fn post(url: String) -> HttpRequest {
    HttpRequest::post(url).body(b"hi".to_vec())
}

#[test]
fn default_client_follows_redirects() {
    let server = Server::start();
    let response = block_on(ReqwestClient::new().execute(post(server.url("/hook")))).unwrap();

    assert_eq!(response.status, 200);
    // The POST is resent as a bodyless GET, as in Go.
    assert_eq!(server.seen(), ["POST /hook", "GET /landed"]);
}

#[test]
fn reject_policy_fails_the_request_without_following() {
    let server = Server::start();
    let http = ReqwestClient::with_redirect_policy(RedirectPolicy::Reject);
    let err = block_on(http.execute(post(server.url("/hook"))))
        .unwrap_err()
        .to_string();

    assert!(err.contains("HTTP 302 to /landed"), "{err}");
    assert_eq!(server.seen(), ["POST /hook"]);
}

#[test]
fn reject_policy_fails_a_redirect_without_location() {
    let server = Server::start();
    let http = ReqwestClient::with_redirect_policy(RedirectPolicy::Reject);
    let err = block_on(http.execute(post(server.url("/no-location"))))
        .unwrap_err()
        .to_string();

    assert!(err.contains("HTTP 302 to no Location header"), "{err}");
}

#[test]
fn reject_policy_still_returns_ordinary_responses() {
    let server = Server::start();
    let http = ReqwestClient::with_redirect_policy(RedirectPolicy::Reject);
    let response = block_on(http.execute(post(server.url("/landed")))).unwrap();

    assert_eq!(response.status, 200);
}

#[cfg(feature = "generic")]
#[test]
fn reject_policy_keeps_generic_from_reporting_a_redirect_as_success() {
    // Generic counts any status below 400 as success, so a redirect it isn't allowed to follow
    // must fail in the transport, before generic sees the 302.
    let server = Server::start();
    let http = ReqwestClient::with_redirect_policy(RedirectPolicy::Reject);
    let url = format!("generic+{}", server.url("/hook"));
    let err = block_on(shoutrrr::send(&http, &url, "hi"))
        .unwrap_err()
        .to_string();

    assert!(err.contains("HTTP 302"), "{err}");
    assert_eq!(server.seen(), ["POST /hook"]);
}
