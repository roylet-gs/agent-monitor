//! The OTLP exporters' HTTP client, over the blocking `ureq` the integrations
//! already use, so no async runtime enters the build. The exporters call it
//! from their own batch threads and poll the future at once, so doing the
//! request before returning a ready future blocks nothing that matters.

use std::future::{self, Future};
use std::pin::Pin;
use std::time::Duration;

use opentelemetry_http::{Bytes, HttpClient, HttpError, Request, Response};

/// Responses are a status and a small body; anything bigger is refused.
const MAX_RESPONSE_BYTES: u64 = 4 * 1024 * 1024;

#[derive(Debug)]
pub struct UreqClient {
    agent: ureq::Agent,
}

impl UreqClient {
    /// `timeout` bounds the whole request, body included.
    pub fn new(timeout: Duration) -> Self {
        let agent = ureq::Agent::config_builder()
            .timeout_global(Some(timeout))
            // The exporter reads the status itself (and retries 429/5xx).
            .http_status_as_error(false)
            .build()
            .into();
        UreqClient { agent }
    }

    pub fn send(&self, request: Request<Bytes>) -> Result<Response<Bytes>, HttpError> {
        let (parts, body) = request.into_parts();
        let response = self.agent.run(Request::from_parts(parts, body.to_vec()))?;
        let (parts, body) = response.into_parts();
        let bytes = body.into_with_config().limit(MAX_RESPONSE_BYTES).read_to_vec()?;
        Ok(Response::from_parts(parts, Bytes::from(bytes)))
    }
}

// `HttpClient` is an `#[async_trait]`; this is its desugared signature.
impl HttpClient for UreqClient {
    fn send_bytes<'life0, 'async_trait>(
        &'life0 self,
        request: Request<Bytes>,
    ) -> Pin<Box<dyn Future<Output = Result<Response<Bytes>, HttpError>> + Send + 'async_trait>>
    where
        Self: 'async_trait,
        'life0: 'async_trait,
    {
        Box::pin(future::ready(self.send(request)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;

    #[test]
    fn posts_the_body_and_returns_non_success_statuses() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/v1/logs", listener.local_addr().unwrap());
        let server = std::thread::spawn(move || {
            let (mut conn, _) = listener.accept().unwrap();
            let mut seen = Vec::new();
            let mut buf = [0; 4096];
            // Headers and body may arrive in separate reads.
            while !seen.ends_with(b"payload") {
                let n = conn.read(&mut buf).unwrap();
                assert!(n > 0, "connection closed early");
                seen.extend_from_slice(&buf[..n]);
            }
            conn.write_all(b"HTTP/1.1 503 Service Unavailable\r\nContent-Length: 4\r\n\r\nbusy").unwrap();
            String::from_utf8_lossy(&seen).into_owned()
        });
        let request = Request::post(url)
            .header("content-type", "application/x-protobuf")
            .body(Bytes::from_static(b"payload"))
            .unwrap();
        let response = UreqClient::new(Duration::from_secs(5)).send(request).unwrap();
        assert_eq!(response.status(), 503);
        assert_eq!(response.body().as_ref(), b"busy");
        let seen = server.join().unwrap();
        assert!(seen.starts_with("POST /v1/logs"), "{seen}");
        assert!(seen.ends_with("payload"), "{seen}");
    }

    #[test]
    fn an_unreachable_collector_is_an_error_not_a_hang() {
        // Bind then drop, so the port is closed.
        let addr = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap();
        let request = Request::post(format!("http://{addr}/v1/logs")).body(Bytes::new()).unwrap();
        assert!(UreqClient::new(Duration::from_secs(2)).send(request).is_err());
    }
}
