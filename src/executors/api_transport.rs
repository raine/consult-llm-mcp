use std::io::Read;
use std::time::Duration;

use super::sse::{SseEvent, SseParser};

/// Error-message labels for one provider's transport. Kept short and
/// provider-specific so log lines remain greppable per-provider.
#[derive(Clone, Copy)]
pub struct StreamLabels {
    /// Used in `"{request} to {model} failed: ..."` and
    /// `"{request} failed with status ..."`.
    pub request: &'static str,
    /// Used in `"{stream} idle timeout: ..."`, `"{stream} error for ..."`,
    /// and `"{stream} parse error for ..."`.
    pub stream: &'static str,
}

/// Inputs to a streaming POST request. The transport owns the wire phase
/// (send + status check + SSE read loop + idle-timeout error mapping); the
/// caller owns body construction and per-event decoding.
pub struct StreamRequest<'a> {
    pub agent: &'a ureq::Agent,
    pub url: String,
    pub headers: Vec<(&'static str, String)>,
    pub body: Vec<u8>,
    pub idle_timeout: Duration,
    pub model: String,
    pub labels: StreamLabels,
}

/// Per-event decoder. Returning `Ok(true)` ends the stream loop early.
/// Called for every event surfaced by `SseParser::feed` and once more for
/// any event emitted by `SseParser::flush()` after EOF.
pub trait EventHandler {
    fn on_event(&mut self, ev: &SseEvent) -> anyhow::Result<bool>;
}

pub fn run_stream<H: EventHandler>(req: StreamRequest, handler: &mut H) -> anyhow::Result<()> {
    let StreamRequest {
        agent,
        url,
        headers,
        body,
        idle_timeout,
        model,
        labels,
    } = req;

    let idle_secs = idle_timeout.as_secs();
    let mut builder = agent
        .post(&url)
        .config()
        // Per-read socket idle: ureq applies this as a fresh budget on every
        // read, so heartbeat bytes (and any data, parsed or not) reset the
        // timer. The agent-level timeout_global bounds the whole request as
        // an absolute backstop.
        .timeout_recv_body(Some(idle_timeout))
        .http_status_as_error(false)
        .build();
    for (k, v) in headers {
        builder = builder.header(k, v);
    }

    let resp = builder.send(&body[..]);
    let mut resp = match resp {
        Ok(r) => r,
        Err(e) => anyhow::bail!("{} to {model} failed: {e}", labels.request),
    };

    if !resp.status().is_success() {
        let status = resp.status();
        let body_text = resp.body_mut().read_to_string().unwrap_or_default();
        anyhow::bail!(
            "{} failed with status {status}: {body_text}",
            labels.request
        );
    }

    let mut reader = resp.into_body().into_reader();
    let mut sse = SseParser::new();
    let mut buf = [0u8; 8192];

    'outer: loop {
        let n = match reader.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) => {
                if is_timeout_err(&e) {
                    anyhow::bail!(
                        "{} idle timeout: no bytes from {model} for {idle_secs}s",
                        labels.stream
                    );
                }
                anyhow::bail!("{} error for {model}: {e}", labels.stream);
            }
        };
        let events = sse
            .feed(&buf[..n])
            .map_err(|e| anyhow::anyhow!("{} parse error for {model}: {e}", labels.stream))?;
        for ev in events {
            if handler.on_event(&ev)? {
                break 'outer;
            }
        }
    }
    if let Some(ev) = sse.flush() {
        let _ = handler.on_event(&ev)?;
    }
    Ok(())
}

/// True if the IO error originated from a ureq timeout (or stdlib TimedOut).
/// `timeout_recv_body` raises a `ureq::Error::Timeout` which surfaces as
/// `io::Error` with kind `TimedOut` when reading from `BodyReader`.
fn is_timeout_err(e: &std::io::Error) -> bool {
    if e.kind() == std::io::ErrorKind::TimedOut {
        return true;
    }
    let s = e.to_string();
    s.contains("timeout") || s.contains("Timeout")
}

#[cfg(test)]
mod tests {
    //! End-to-end mock-server tests for the shared API transport. Both the
    //! OpenAI-compatible (`ApiExecutor`) and Anthropic (`AnthropicApiExecutor`)
    //! executors are pointed at an in-process TCP listener and exercised
    //! through a real `ureq::Agent`. We verify (a) the request body the
    //! shared transport sends matches a recorded golden for each provider,
    //! and (b) decoding a recorded response stream produces the expected
    //! `ExecuteResult`.

    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::{Arc, Mutex};
    use std::thread;

    use consult_llm_core::monitoring::RunSpool;

    use super::super::anthropic_api::AnthropicApiExecutor;
    use super::super::api::ApiExecutor;
    use super::super::types::{ExecutionRequest, LlmExecutor};

    /// Spin up a one-shot HTTP/1.1 server on 127.0.0.1 that accepts a single
    /// POST, records the request body, and replies with `response_bytes`.
    /// Returns `(base_url, recorded_body_handle)`.
    fn start_mock_server(response_bytes: Vec<u8>) -> (String, Arc<Mutex<Vec<u8>>>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().unwrap().port();
        let recorded = Arc::new(Mutex::new(Vec::<u8>::new()));
        let recorded_clone = Arc::clone(&recorded);

        thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept");
            stream
                .set_read_timeout(Some(std::time::Duration::from_secs(5)))
                .ok();

            let mut all = Vec::<u8>::new();
            let mut buf = [0u8; 4096];
            // Read until headers complete + we have the full body per
            // Content-Length. The executors always send Content-Length
            // (no chunked uploads).
            let body = loop {
                let n = match stream.read(&mut buf) {
                    Ok(0) | Err(_) => break Vec::new(),
                    Ok(n) => n,
                };
                all.extend_from_slice(&buf[..n]);
                if let Some(hdr_end) = find_subslice(&all, b"\r\n\r\n") {
                    let headers = std::str::from_utf8(&all[..hdr_end]).unwrap_or("");
                    let cl: usize = headers
                        .lines()
                        .find_map(|l| {
                            let (k, v) = l.split_once(':')?;
                            (k.eq_ignore_ascii_case("content-length"))
                                .then(|| v.trim().parse::<usize>().ok())
                                .flatten()
                        })
                        .unwrap_or(0);
                    let body_start = hdr_end + 4;
                    while all.len() - body_start < cl {
                        let n = match stream.read(&mut buf) {
                            Ok(0) | Err(_) => break,
                            Ok(n) => n,
                        };
                        all.extend_from_slice(&buf[..n]);
                    }
                    break all[body_start..body_start + cl].to_vec();
                }
            };
            *recorded_clone.lock().unwrap() = body;

            let _ = stream.write_all(&response_bytes);
            let _ = stream.flush();
            // Half-close so ureq sees EOF and stops reading.
            let _ = stream.shutdown(std::net::Shutdown::Write);
        });

        (format!("http://127.0.0.1:{port}"), recorded)
    }

    fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
        if needle.is_empty() || haystack.len() < needle.len() {
            return None;
        }
        haystack.windows(needle.len()).position(|w| w == needle)
    }

    fn http_response(body: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(b"HTTP/1.1 200 OK\r\n");
        out.extend_from_slice(b"Content-Type: text/event-stream\r\n");
        out.extend_from_slice(b"Connection: close\r\n");
        out.extend_from_slice(b"\r\n");
        out.extend_from_slice(body);
        out
    }

    fn isolate_state_dir() -> tempfile::TempDir {
        // Test runs may persist stored threads via ApiChatSession::commit_turn.
        // Pin XDG_STATE_HOME to a tempdir so we never write to the user's real
        // state directory. The lock keeps us serialized with any other test
        // in this crate that mutates XDG_STATE_HOME — see `test_util`.
        let tmp = tempfile::tempdir().unwrap();
        unsafe {
            std::env::set_var("XDG_STATE_HOME", tmp.path());
        }
        tmp
    }

    fn build_request(prompt: &str, model: &str) -> (ExecutionRequest, Arc<Mutex<RunSpool>>) {
        let spool = Arc::new(Mutex::new(RunSpool::disabled()));
        let req = ExecutionRequest {
            prompt: prompt.to_string(),
            model: model.to_string(),
            system_prompt: "you are helpful".to_string(),
            file_paths: None,
            thread_id: None,
            spool: Arc::clone(&spool),
        };
        (req, spool)
    }

    #[test]
    fn mock_server_openai_and_anthropic_executors() {
        let _xdg_guard = crate::test_util::XDG_STATE_LOCK
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        let _state = isolate_state_dir();

        // --- OpenAI-compat provider -----------------------------------
        let openai_sse = b"\
data: {\"choices\":[{\"delta\":{\"content\":\"Hello\"}}]}\n\n\
data: {\"choices\":[{\"delta\":{\"content\":\" world\"},\"finish_reason\":\"stop\"}]}\n\n\
data: {\"choices\":[],\"usage\":{\"prompt_tokens\":7,\"completion_tokens\":2}}\n\n\
data: [DONE]\n\n\
";
        let (openai_base, openai_recorded) = start_mock_server(http_response(openai_sse));
        let agent = ureq::Agent::new_with_defaults();
        let openai = ApiExecutor::new(
            agent.clone(),
            "test-openai-key".to_string(),
            Some(format!("{openai_base}/v1/")),
            std::time::Duration::from_secs(5),
        );
        let (req, _spool) = build_request("hi there", "gpt-test");
        let result = openai.execute(req).expect("openai execute");
        assert_eq!(result.response, "Hello world");
        let usage = result.usage.expect("usage");
        assert_eq!(usage.prompt_tokens, 7);
        assert_eq!(usage.completion_tokens, 2);

        let recorded: serde_json::Value =
            serde_json::from_slice(&openai_recorded.lock().unwrap()).expect("openai req json");
        let expected_openai = serde_json::json!({
            "model": "gpt-test",
            "messages": [
                {"role": "system", "content": "you are helpful"},
                {"role": "user", "content": "hi there"},
            ],
            "stream": true,
            "stream_options": {"include_usage": true},
        });
        assert_eq!(recorded, expected_openai);

        // --- Anthropic provider --------------------------------------
        let anthropic_sse = b"\
data: {\"type\":\"message_start\",\"message\":{\"usage\":{\"input_tokens\":11,\"cache_creation_input_tokens\":0,\"cache_read_input_tokens\":0}}}\n\n\
data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\"}}\n\n\
data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"hi \"}}\n\n\
data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"there\"}}\n\n\
data: {\"type\":\"content_block_stop\",\"index\":0}\n\n\
data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":5}}\n\n\
data: {\"type\":\"message_stop\"}\n\n\
";
        let (anthropic_base, anthropic_recorded) = start_mock_server(http_response(anthropic_sse));
        let anthropic = AnthropicApiExecutor::new(
            agent,
            "test-anthropic-key".to_string(),
            Some(anthropic_base),
            std::time::Duration::from_secs(5),
        );
        let (req, _spool) = build_request("hello?", "claude-test");
        let result = anthropic.execute(req).expect("anthropic execute");
        assert_eq!(result.response, "hi there");
        let usage = result.usage.expect("anthropic usage");
        assert_eq!(usage.prompt_tokens, 11);
        assert_eq!(usage.completion_tokens, 5);

        let recorded: serde_json::Value =
            serde_json::from_slice(&anthropic_recorded.lock().unwrap()).expect("anth req json");
        let expected_anthropic = serde_json::json!({
            "model": "claude-test",
            "system": "you are helpful",
            "messages": [{"role": "user", "content": "hello?"}],
            "max_tokens": 32_000,
            "stream": true,
        });
        assert_eq!(recorded, expected_anthropic);
    }
}
