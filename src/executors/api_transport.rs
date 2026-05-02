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

pub(super) struct PreparedStreamRequest {
    pub url: String,
    pub headers: Vec<(&'static str, String)>,
    pub body: Vec<u8>,
    pub idle_timeout: Duration,
    pub model: String,
    pub labels: StreamLabels,
}

impl PreparedStreamRequest {
    pub fn into_stream_request(self, agent: &ureq::Agent) -> StreamRequest<'_> {
        StreamRequest {
            agent,
            url: self.url,
            headers: self.headers,
            body: self.body,
            idle_timeout: self.idle_timeout,
            model: self.model,
            labels: self.labels,
        }
    }
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
pub trait StreamHandler {
    type Outcome;

    fn on_event(&mut self, ev: &SseEvent) -> anyhow::Result<bool>;
    fn finish(self, model: &str) -> anyhow::Result<Self::Outcome>;
}

pub fn run_stream<H: StreamHandler>(req: StreamRequest, handler: H) -> anyhow::Result<H::Outcome> {
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

    drive_stream_reader(
        resp.into_body().into_reader(),
        handler,
        &model,
        labels,
        idle_secs,
    )
}

fn drive_stream_reader<H: StreamHandler, R: Read>(
    reader: R,
    handler: H,
    model: &str,
    labels: StreamLabels,
    idle_secs: u64,
) -> anyhow::Result<H::Outcome> {
    drive_stream_reader_with_buf(reader, handler, model, labels, idle_secs, 8192)
}

fn drive_stream_reader_with_buf<H: StreamHandler, R: Read>(
    mut reader: R,
    mut handler: H,
    model: &str,
    labels: StreamLabels,
    idle_secs: u64,
    buf_size: usize,
) -> anyhow::Result<H::Outcome> {
    let mut sse = SseParser::new();
    let mut buf = vec![0u8; buf_size];

    let mut stopped = false;
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
                stopped = true;
                break 'outer;
            }
        }
    }
    if !stopped && let Some(ev) = sse.flush() {
        let _ = handler.on_event(&ev)?;
    }
    handler.finish(model)
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

    #[cfg(all(feature = "net-tests", not(feature = "no-net-tests")))]
    use std::io::Write;
    use std::io::{Cursor, ErrorKind, Read};
    #[cfg(all(feature = "net-tests", not(feature = "no-net-tests")))]
    use std::net::TcpListener;
    #[cfg(all(feature = "net-tests", not(feature = "no-net-tests")))]
    use std::sync::Arc;
    use std::sync::Mutex;
    #[cfg(all(feature = "net-tests", not(feature = "no-net-tests")))]
    use std::thread;

    use consult_llm_core::monitoring::RunSpool;
    #[cfg(all(feature = "net-tests", not(feature = "no-net-tests")))]
    use consult_llm_core::monitoring::{RunEvent, RunEventKind, RunMeta};
    #[cfg(all(feature = "net-tests", not(feature = "no-net-tests")))]
    use consult_llm_core::stream_events::ParsedStreamEvent;

    use super::super::anthropic_api::AnthropicApiExecutor;
    use super::super::anthropic_events::AnthropicStreamHandler;
    use super::super::api::ApiExecutor;
    use super::super::api_chat::ChatStreamHandler;
    #[cfg(all(feature = "net-tests", not(feature = "no-net-tests")))]
    use super::super::types::{ExecutionRequest, LlmExecutor};
    use super::*;
    use crate::models::{ApiProtocol, Provider};

    /// Spin up a one-shot HTTP/1.1 server on 127.0.0.1 that accepts a single
    /// POST, records the request body, and replies with `response_bytes`.
    /// Returns `(base_url, recorded_body_handle)`.
    #[cfg(all(feature = "net-tests", not(feature = "no-net-tests")))]
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

    #[cfg(all(feature = "net-tests", not(feature = "no-net-tests")))]
    fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
        if needle.is_empty() || haystack.len() < needle.len() {
            return None;
        }
        haystack.windows(needle.len()).position(|w| w == needle)
    }

    #[cfg(all(feature = "net-tests", not(feature = "no-net-tests")))]
    fn http_response(body: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(b"HTTP/1.1 200 OK\r\n");
        out.extend_from_slice(b"Content-Type: text/event-stream\r\n");
        out.extend_from_slice(b"Connection: close\r\n");
        out.extend_from_slice(b"\r\n");
        out.extend_from_slice(body);
        out
    }

    struct RecordingHandler {
        seen: Vec<String>,
        stop_on: &'static str,
    }

    impl super::StreamHandler for RecordingHandler {
        type Outcome = Vec<String>;

        fn on_event(&mut self, ev: &SseEvent) -> anyhow::Result<bool> {
            self.seen.push(ev.data.clone());
            Ok(ev.data == self.stop_on)
        }

        fn finish(self, _model: &str) -> anyhow::Result<Self::Outcome> {
            Ok(self.seen)
        }
    }

    fn test_labels() -> StreamLabels {
        StreamLabels {
            request: "test request",
            stream: "test stream",
        }
    }

    #[test]
    fn run_stream_honors_terminal_event_from_eof_flush() {
        let seen = drive_stream_reader(
            Cursor::new(b"data: stop"),
            RecordingHandler {
                seen: Vec::new(),
                stop_on: "stop",
            },
            "test-model",
            test_labels(),
            5,
        )
        .unwrap();
        assert_eq!(seen, vec!["stop"]);
    }

    #[test]
    fn run_stream_skips_flush_after_in_loop_stop() {
        let seen = drive_stream_reader(
            Cursor::new(b"data: stop\n\ndata: later"),
            RecordingHandler {
                seen: Vec::new(),
                stop_on: "stop",
            },
            "test-model",
            test_labels(),
            5,
        )
        .unwrap();
        assert_eq!(seen, vec!["stop"]);
    }

    struct FailingReader {
        kind: ErrorKind,
        message: &'static str,
    }

    impl Read for FailingReader {
        fn read(&mut self, _buf: &mut [u8]) -> std::io::Result<usize> {
            Err(std::io::Error::new(self.kind, self.message))
        }
    }

    struct OversizedFrameReader {
        remaining: usize,
    }

    impl Read for OversizedFrameReader {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            if self.remaining == 0 {
                return Ok(0);
            }
            let n = self.remaining.min(buf.len());
            buf[..n].fill(b'x');
            self.remaining -= n;
            Ok(n)
        }
    }

    #[test]
    fn drive_stream_reader_labels_parse_errors() {
        let err = drive_stream_reader_with_buf(
            OversizedFrameReader {
                remaining: 17 * 1024 * 1024,
            },
            RecordingHandler {
                seen: Vec::new(),
                stop_on: "stop",
            },
            "test-model",
            test_labels(),
            5,
            17 * 1024 * 1024,
        )
        .unwrap_err()
        .to_string();
        assert!(err.starts_with("test stream parse error for test-model: "));
    }

    #[test]
    fn drive_stream_reader_labels_timeout_errors() {
        let err = drive_stream_reader(
            FailingReader {
                kind: ErrorKind::TimedOut,
                message: "timed out",
            },
            RecordingHandler {
                seen: Vec::new(),
                stop_on: "stop",
            },
            "test-model",
            test_labels(),
            5,
        )
        .unwrap_err()
        .to_string();
        assert_eq!(
            err,
            "test stream idle timeout: no bytes from test-model for 5s"
        );
    }

    #[test]
    fn drive_stream_reader_labels_generic_read_errors() {
        let err = drive_stream_reader(
            FailingReader {
                kind: ErrorKind::Other,
                message: "broken pipe",
            },
            RecordingHandler {
                seen: Vec::new(),
                stop_on: "stop",
            },
            "test-model",
            test_labels(),
            5,
        )
        .unwrap_err()
        .to_string();
        assert_eq!(err, "test stream error for test-model: broken pipe");
    }

    fn openai_sse() -> &'static [u8] {
        b"\
data: {\"choices\":[{\"delta\":{\"content\":\"Hello\"}}]}\n\n\
data: {\"choices\":[{\"delta\":{\"content\":\" world\"},\"finish_reason\":\"stop\"}]}\n\n\
data: {\"choices\":[],\"usage\":{\"prompt_tokens\":7,\"completion_tokens\":2}}\n\n\
data: [DONE]\n\n\
"
    }

    fn anthropic_sse() -> &'static [u8] {
        b"\
data: {\"type\":\"message_start\",\"message\":{\"usage\":{\"input_tokens\":11,\"cache_creation_input_tokens\":0,\"cache_read_input_tokens\":0}}}\n\n\
data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\"}}\n\n\
data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"hi \"}}\n\n\
data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"there\"}}\n\n\
data: {\"type\":\"content_block_stop\",\"index\":0}\n\n\
data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":5}}\n\n\
data: {\"type\":\"message_stop\"}\n\n\
"
    }

    #[cfg(all(feature = "net-tests", not(feature = "no-net-tests")))]
    fn build_request(prompt: &str, model: &str) -> (ExecutionRequest, Arc<Mutex<RunSpool>>) {
        build_request_with_spool(prompt, model, RunSpool::disabled())
    }

    #[cfg(all(feature = "net-tests", not(feature = "no-net-tests")))]
    fn build_request_with_spool(
        prompt: &str,
        model: &str,
        spool: RunSpool,
    ) -> (ExecutionRequest, Arc<Mutex<RunSpool>>) {
        let spool = Arc::new(Mutex::new(spool));
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

    fn runtime_for(provider: Provider) -> crate::models::OpenAiCompatRuntime {
        let ApiProtocol::OpenAiCompat(runtime) = provider.api_protocol() else {
            panic!("{provider:?} is not OpenAI-compatible");
        };
        runtime
    }

    #[cfg(all(feature = "net-tests", not(feature = "no-net-tests")))]
    fn active_spool(model: &str) -> (crate::test_util::XdgStateGuard, RunSpool) {
        let state = crate::test_util::XdgStateGuard::temp();
        let spool = RunSpool::new(RunMeta {
            v: 1,
            run_id: format!("run-{model}"),
            pid: std::process::id(),
            started_at: "t".into(),
            project: "p".into(),
            cwd: "/tmp".into(),
            model: model.into(),
            backend: "api".into(),
            thread_id: None,
            task_mode: None,
            reasoning_effort: None,
        });
        (state, spool)
    }

    #[cfg(all(feature = "net-tests", not(feature = "no-net-tests")))]
    fn recorded_thinking_events(run_id: &str) -> Vec<String> {
        let path = consult_llm_core::monitoring::runs_dir().join(format!("{run_id}.events.jsonl"));
        let events = std::fs::read_to_string(path).unwrap();
        events
            .lines()
            .filter_map(|line| serde_json::from_str::<RunEvent>(line).ok())
            .filter_map(|event| match event.kind {
                RunEventKind::Stream {
                    event: ParsedStreamEvent::Thinking { text },
                } => Some(text),
                _ => None,
            })
            .collect()
    }

    #[cfg(all(feature = "net-tests", not(feature = "no-net-tests")))]
    #[test]
    fn mock_server_gemini_runtime_metadata_ignores_base_url() {
        let (state, spool) = active_spool("gemini-2.5-pro");

        let gemini_sse = b"\
data: {\"choices\":[{\"delta\":{\"content\":\"<thought>check\"}}]}\n\n\
data: {\"choices\":[{\"delta\":{\"content\":\"ing</thought>Hello\"}}]}\n\n\
data: {\"choices\":[{\"delta\":{\"content\":\" world\"},\"finish_reason\":\"stop\"}]}\n\n\
data: [DONE]\n\n\
";
        let (base, recorded) = start_mock_server(http_response(gemini_sse));
        let gemini = ApiExecutor::new(
            ureq::Agent::new_with_defaults(),
            "test-gemini-key".to_string(),
            Some(format!("{base}/custom/")),
            std::time::Duration::from_secs(5),
            runtime_for(Provider::Gemini),
        );
        let (req, _spool) = build_request_with_spool("hi", "gemini-2.5-pro", spool);
        let result = gemini.execute(req).expect("gemini execute");
        assert_eq!(result.response, "Hello world");

        let body: serde_json::Value =
            serde_json::from_slice(&recorded.lock().unwrap()).expect("gemini req json");
        assert_eq!(
            body["extra_body"],
            serde_json::json!({
                "google": {
                    "thinking_config": {
                        "thinking_level": "high",
                        "include_thoughts": true
                    }
                }
            })
        );
        assert_eq!(
            recorded_thinking_events("run-gemini-2.5-pro"),
            vec!["check", "ing"]
        );
        drop(state);
    }

    #[cfg(all(feature = "net-tests", not(feature = "no-net-tests")))]
    #[test]
    fn mock_server_minimax_runtime_metadata_ignores_base_url() {
        let (state, spool) = active_spool("MiniMax-M2.7");

        let minimax_sse = b"\
data: {\"choices\":[{\"delta\":{\"content\":\"<think>plan</think>Answer\"}}]}\n\n\
data: {\"choices\":[{\"delta\":{\"content\":\" done\"},\"finish_reason\":\"stop\"}]}\n\n\
data: [DONE]\n\n\
";
        let (base, recorded) = start_mock_server(http_response(minimax_sse));
        let minimax = ApiExecutor::new(
            ureq::Agent::new_with_defaults(),
            "test-minimax-key".to_string(),
            Some(format!("{base}/custom/")),
            std::time::Duration::from_secs(5),
            runtime_for(Provider::MiniMax),
        );
        let (req, _spool) = build_request_with_spool("hi", "MiniMax-M2.7", spool);
        let result = minimax.execute(req).expect("minimax execute");
        assert_eq!(result.response, "Answer done");

        let body: serde_json::Value =
            serde_json::from_slice(&recorded.lock().unwrap()).expect("minimax req json");
        assert!(body.get("extra_body").is_none());
        assert_eq!(recorded_thinking_events("run-MiniMax-M2.7"), vec!["plan"]);
        drop(state);
    }

    #[test]
    fn offline_openai_request_golden() {
        let agent = ureq::Agent::new_with_defaults();
        let openai = ApiExecutor::new(
            agent,
            "test-openai-key".to_string(),
            Some("http://example.test/v1/".to_string()),
            std::time::Duration::from_secs(5),
            runtime_for(Provider::OpenAI),
        );
        let prepared = openai
            .build_stream_request(
                "gpt-test".to_string(),
                "you are helpful",
                "hi there",
                std::iter::empty(),
            )
            .unwrap();

        assert_eq!(prepared.url, "http://example.test/v1/chat/completions");
        assert_eq!(
            prepared.headers,
            vec![
                ("Authorization", "Bearer test-openai-key".to_string()),
                ("Content-Type", "application/json".to_string()),
            ]
        );
        let recorded: serde_json::Value = serde_json::from_slice(&prepared.body).unwrap();
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
    }

    #[test]
    fn offline_anthropic_request_golden() {
        let agent = ureq::Agent::new_with_defaults();
        let anthropic = AnthropicApiExecutor::new(
            agent,
            "test-anthropic-key".to_string(),
            Some("http://example.test".to_string()),
            std::time::Duration::from_secs(5),
        );
        let prepared = anthropic
            .build_stream_request(
                "claude-test".to_string(),
                "you are helpful".to_string(),
                "hello?".to_string(),
                std::iter::empty(),
            )
            .unwrap();

        assert_eq!(prepared.url, "http://example.test/v1/messages");
        assert_eq!(
            prepared.headers,
            vec![
                ("x-api-key", "test-anthropic-key".to_string()),
                ("anthropic-version", "2023-06-01".to_string()),
                ("Content-Type", "application/json".to_string()),
            ]
        );
        let recorded: serde_json::Value = serde_json::from_slice(&prepared.body).unwrap();
        let expected_anthropic = serde_json::json!({
            "model": "claude-test",
            "system": "you are helpful",
            "messages": [{"role": "user", "content": "hello?"}],
            "max_tokens": 32_000,
            "stream": true,
        });
        assert_eq!(recorded, expected_anthropic);
    }

    #[test]
    fn offline_openai_response_golden() {
        let spool = Mutex::new(RunSpool::disabled());
        let outcome = drive_stream_reader(
            Cursor::new(openai_sse()),
            ChatStreamHandler::new(None, &spool),
            "gpt-test",
            test_labels(),
            5,
        )
        .unwrap();

        assert_eq!(outcome.response, "Hello world");
        let usage = outcome.usage.unwrap();
        assert_eq!(usage.prompt_tokens, 7);
        assert_eq!(usage.completion_tokens, 2);
    }

    #[test]
    fn offline_anthropic_response_golden() {
        let spool = Mutex::new(RunSpool::disabled());
        let outcome = drive_stream_reader(
            Cursor::new(anthropic_sse()),
            AnthropicStreamHandler::new(&spool, 32_000),
            "claude-test",
            test_labels(),
            5,
        )
        .unwrap();

        assert_eq!(outcome.response, "hi there");
        let usage = outcome.usage.unwrap();
        assert_eq!(usage.prompt_tokens, 11);
        assert_eq!(usage.completion_tokens, 5);
    }

    #[cfg(all(feature = "net-tests", not(feature = "no-net-tests")))]
    #[test]
    fn mock_server_openai_and_anthropic_executors() {
        let _state = crate::test_util::XdgStateGuard::temp();

        let (openai_base, openai_recorded) = start_mock_server(http_response(openai_sse()));
        let agent = ureq::Agent::new_with_defaults();
        let openai = ApiExecutor::new(
            agent.clone(),
            "test-openai-key".to_string(),
            Some(format!("{openai_base}/v1/")),
            std::time::Duration::from_secs(5),
            runtime_for(Provider::OpenAI),
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

        let (anthropic_base, anthropic_recorded) =
            start_mock_server(http_response(anthropic_sse()));
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
