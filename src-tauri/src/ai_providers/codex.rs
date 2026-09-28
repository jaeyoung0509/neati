use super::registry::ProviderRegistry;
use super::support::{append_rate_windows, base_provider, u64_field};
use super::{CollectionContext, ProviderAdapter, ProviderDescriptor, ProviderError};
use crate::models::{AiProviderUsage, ProviderId, UsageCollectionStatus};
use crate::tooling;
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Child, ChildStdin, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

const DEADLINE: Duration = Duration::from_secs(12);
const MAX_LINE_BYTES: u64 = 256 * 1024;
const MAX_MESSAGES: usize = 128;

#[derive(Default)]
pub struct CodexAdapter;

impl ProviderAdapter for CodexAdapter {
    fn id(&self) -> ProviderId {
        ProviderId::Codex
    }
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderRegistry::find(self.id())
            .expect("registered Codex")
            .to_descriptor()
    }
    fn collect(&self, ctx: &CollectionContext<'_>) -> Result<AiProviderUsage, ProviderError> {
        self.collect_with_progress(ctx, &|_| {})
    }
    fn collect_with_progress(
        &self,
        _ctx: &CollectionContext<'_>,
        on_progress: &dyn Fn(AiProviderUsage),
    ) -> Result<AiProviderUsage, ProviderError> {
        let mut transport = ProcessTransport::start()?;
        collect_protocol_with_progress(&mut transport, Instant::now() + DEADLINE, on_progress)
    }
}

trait Transport {
    fn send(&mut self, message: Value) -> Result<(), ProviderError>;
    fn receive(&mut self, deadline: Instant) -> Result<Value, ProviderError>;
}

struct ProcessTransport {
    child: Child,
    input: Option<ChildStdin>,
    receiver: Receiver<Result<Value, ProviderError>>,
    reader: Option<std::thread::JoinHandle<()>>,
}

impl ProcessTransport {
    fn start() -> Result<Self, ProviderError> {
        // stdio is the documented default, including older supported CLIs.
        let mut child = tooling::command("codex")
            .arg("app-server")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|error| {
                if error.kind() == std::io::ErrorKind::NotFound {
                    ProviderError::CliNotInstalled(
                        "Install Codex CLI to read account usage.".into(),
                    )
                } else {
                    ProviderError::CliFailed("Could not start Codex app-server.".into())
                }
            })?;
        let stdout = child.stdout.take().expect("piped stdout");
        let input = child.stdin.take();
        let (tx, receiver) = mpsc::sync_channel(32);
        let reader = std::thread::spawn(move || {
            let mut stream = BufReader::new(stdout);
            loop {
                let message = read_message(&mut stream);
                let failed = message.is_err();
                // Bounded queue, nonblocking teardown. Overflow fails closed.
                if tx.try_send(message).is_err() || failed {
                    break;
                }
            }
        });
        Ok(Self {
            child,
            input,
            receiver,
            reader: Some(reader),
        })
    }
}

fn read_message(reader: &mut impl BufRead) -> Result<Value, ProviderError> {
    let mut bytes = Vec::new();
    reader
        .take(MAX_LINE_BYTES + 1)
        .read_until(b'\n', &mut bytes)
        .map_err(|_| ProviderError::CliFailed("Codex response could not be read.".into()))?;
    if bytes.is_empty() {
        return Err(ProviderError::CliFailed(
            "Codex exited before completing usage collection.".into(),
        ));
    }
    if bytes.len() as u64 > MAX_LINE_BYTES {
        return Err(protocol_error("Codex response exceeded the size limit."));
    }
    serde_json::from_slice(&bytes).map_err(|_| protocol_error("Codex returned malformed JSON."))
}

impl Transport for ProcessTransport {
    fn send(&mut self, message: Value) -> Result<(), ProviderError> {
        let input = self
            .input
            .as_mut()
            .ok_or_else(|| protocol_error("Codex input closed."))?;
        writeln!(input, "{message}")
            .and_then(|()| input.flush())
            .map_err(|_| ProviderError::CliFailed("Could not send a Codex usage request.".into()))
    }
    fn receive(&mut self, deadline: Instant) -> Result<Value, ProviderError> {
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .ok_or(ProviderError::Timeout)?;
        self.receiver
            .recv_timeout(remaining)
            .map_err(|error| match error {
                mpsc::RecvTimeoutError::Timeout => ProviderError::Timeout,
                mpsc::RecvTimeoutError::Disconnected => {
                    ProviderError::CliFailed("Codex response stream closed.".into())
                }
            })?
    }
}

impl Drop for ProcessTransport {
    fn drop(&mut self) {
        self.input.take();
        let _ = self.child.kill();
        let _ = self.child.wait();
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
    }
}

fn protocol_error(message: &str) -> ProviderError {
    ProviderError::InvalidResponse(message.into())
}

fn response(
    transport: &mut impl Transport,
    id: u64,
    deadline: Instant,
) -> Result<Value, ProviderError> {
    for _ in 0..MAX_MESSAGES {
        if Instant::now() >= deadline {
            return Err(ProviderError::Timeout);
        }
        let message = transport.receive(deadline)?;
        if message.get("id").and_then(Value::as_u64) != Some(id) {
            continue;
        }
        if message.get("error").is_some() {
            // Raw RPC errors may contain private account data.
            return Err(protocol_error(
                "Codex rejected the usage request. Retry or update Codex CLI.",
            ));
        }
        return message
            .get("result")
            .filter(|value| value.is_object())
            .cloned()
            .ok_or_else(|| protocol_error("Codex response is missing its result."));
    }
    Err(protocol_error("Codex sent too many unrelated messages."))
}

#[cfg(test)]
fn collect_protocol(
    transport: &mut impl Transport,
    deadline: Instant,
) -> Result<AiProviderUsage, ProviderError> {
    collect_protocol_with_progress(transport, deadline, &|_| {})
}

fn collect_protocol_with_progress(
    transport: &mut impl Transport,
    deadline: Instant,
    on_progress: &dyn Fn(AiProviderUsage),
) -> Result<AiProviderUsage, ProviderError> {
    transport.send(json!({"id":0,"method":"initialize","params":{"clientInfo":{"name":"neati","title":"neati","version":env!("CARGO_PKG_VERSION")}}}))?;
    response(transport, 0, deadline)?;
    transport.send(json!({"method":"initialized"}))?;
    transport.send(json!({"id":1,"method":"account/read","params":{"refreshToken":false}}))?;
    let account_result = response(transport, 1, deadline)?;
    let account = account_result
        .get("account")
        .ok_or_else(|| protocol_error("Codex account response is incomplete."))?;
    let mut provider = base_provider(ProviderId::Codex, "Codex", "ChatGPT OAuth");
    provider.installed = true;
    provider.action_url = Some("https://chatgpt.com/codex/settings/usage".into());
    if account.is_null() {
        provider.collection_status = Some(UsageCollectionStatus::SignedOut);
        provider.status_message = "Sign in with ChatGPT using codex login.".into();
        return Ok(provider);
    }
    match account.get("type").and_then(Value::as_str) {
        Some("chatgpt") => {}
        Some("apiKey" | "amazonBedrock") => {
            provider.collection_status = Some(UsageCollectionStatus::UnsupportedAccount);
            provider.status_message =
                "ChatGPT subscription usage requires a ChatGPT login, not API billing.".into();
            return Ok(provider);
        }
        _ => return Err(protocol_error("Codex returned an unknown account type.")),
    }
    transport.send(json!({"id":2,"method":"account/rateLimits/read"}))?;
    let limits = response(transport, 2, deadline)?;
    // Never choose an unrelated model bucket by map iteration order.
    let bucket = limits
        .pointer("/rateLimitsByLimitId/codex")
        .filter(|value| value.is_object())
        .or_else(|| limits.get("rateLimits").filter(|value| value.is_object()))
        .ok_or_else(|| protocol_error("Codex subscription limits are unavailable."))?;
    append_rate_windows(&mut provider.windows, bucket);
    if provider.windows.is_empty() {
        return Err(protocol_error("Codex returned no measured quota windows."));
    }
    // Verify before publishing, without waiting for optional token history.
    // No stable account/workspace identity exists for cross-refresh reuse.
    transport.send(json!({"id":3,"method":"account/read","params":{"refreshToken":false}}))?;
    let verified = response(transport, 3, deadline)?;
    if verified.get("account") != Some(account) {
        return Err(protocol_error(
            "Codex account changed during collection. Refresh usage.",
        ));
    }
    provider.connected = true;
    provider.collection_status = Some(UsageCollectionStatus::Fresh);
    provider.status_message = "Live ChatGPT subscription limits from Codex.".into();
    on_progress(provider.clone());

    // Token activity is optional and is not a quota/authentication prerequisite.
    // Older servers may reject this method. Keep the successful rate limits.
    if transport
        .send(json!({"id":4,"method":"account/usage/read"}))
        .is_err()
    {
        return Ok(provider);
    }
    let optional_deadline = deadline.min(Instant::now() + Duration::from_secs(2));
    if let Ok(usage) = response(transport, 4, optional_deadline) {
        let summary = usage.get("summary").unwrap_or(&Value::Null);
        provider.summary.lifetime_tokens = u64_field(summary, "lifetimeTokens");
        provider.summary.peak_daily_tokens = u64_field(summary, "peakDailyTokens");
        provider.summary.current_streak_days = u64_field(summary, "currentStreakDays");
        provider.summary.last_7d_tokens = usage
            .get("dailyUsageBuckets")
            .and_then(Value::as_array)
            .and_then(|days| {
                days.iter()
                    .rev()
                    .take(7)
                    .filter_map(|day| u64_field(day, "tokens"))
                    .try_fold(0u64, |sum, value| sum.checked_add(value))
            });
    }
    Ok(provider)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;
    struct Fake {
        messages: VecDeque<Result<Value, ProviderError>>,
        sent: Vec<Value>,
        fail_write: bool,
    }
    impl Transport for Fake {
        fn send(&mut self, value: Value) -> Result<(), ProviderError> {
            if self.fail_write {
                return Err(ProviderError::CliFailed("write failed".into()));
            }
            self.sent.push(value);
            Ok(())
        }
        fn receive(&mut self, _: Instant) -> Result<Value, ProviderError> {
            self.messages
                .pop_front()
                .unwrap_or(Err(ProviderError::Timeout))
        }
    }
    fn fixture() -> Fake {
        Fake { sent: vec![], fail_write: false, messages: [
            json!({"id":0,"result":{}}),
            json!({"id":1,"result":{"account":{"type":"chatgpt","email":null,"planType":"plus"}}}),
            json!({"id":2,"result":{"rateLimitsByLimitId":{"other":{"primary":{"usedPercent":99}},"codex":{"primary":{"usedPercent":24,"windowDurationMins":300}}}}}),
            json!({"id":3,"result":{"account":{"type":"chatgpt","email":null,"planType":"plus"}}}),
            json!({"id":4,"error":{"code":-32601,"message":"unsupported optional method"}}),
        ].into_iter().map(Ok).collect() }
    }
    #[test]
    fn initialization_is_acknowledged_before_account_and_codex_bucket_is_selected() {
        let mut transport = fixture();
        let result = collect_protocol(&mut transport, Instant::now() + DEADLINE).unwrap();
        assert!(result.connected);
        assert_eq!(result.windows[0].used_percent, 24.0);
        let methods: Vec<_> = transport
            .sent
            .iter()
            .map(|m| m["method"].as_str().unwrap())
            .collect();
        assert_eq!(
            methods,
            [
                "initialize",
                "initialized",
                "account/read",
                "account/rateLimits/read",
                "account/read",
                "account/usage/read"
            ]
        );
    }
    #[test]
    fn quota_is_published_even_when_optional_history_times_out() {
        use std::cell::Cell;
        let published = Cell::new(false);
        let mut transport = fixture();
        transport.messages[4] = Err(ProviderError::Timeout);
        let result =
            collect_protocol_with_progress(&mut transport, Instant::now() + DEADLINE, &|usage| {
                assert!(usage.connected);
                assert_eq!(usage.windows[0].used_percent, 24.0);
                published.set(true);
            })
            .unwrap();
        assert!(published.get());
        assert_eq!(result.windows[0].used_percent, 24.0);
    }

    #[test]
    fn missing_account_reply_is_timeout_not_signed_out() {
        let mut transport = fixture();
        transport.messages.truncate(1);
        assert_eq!(
            collect_protocol(&mut transport, Instant::now() + DEADLINE).unwrap_err(),
            ProviderError::Timeout
        );
    }
    #[test]
    fn initialization_error_never_sends_account_requests_or_echoes_payload() {
        let mut transport = fixture();
        transport.messages[0] = Ok(json!({"id":0,"error":{"message":"private account payload"}}));
        let error = collect_protocol(&mut transport, Instant::now() + DEADLINE).unwrap_err();
        assert!(!error.to_string().contains("private account payload"));
        assert_eq!(transport.sent.len(), 1);
    }
    #[test]
    fn signed_out_response_has_no_quota_and_needs_no_limits_request() {
        let mut transport = fixture();
        transport.messages[1] = Ok(json!({"id":1,"result":{"account":null}}));
        let result = collect_protocol(&mut transport, Instant::now() + DEADLINE).unwrap();
        assert!(matches!(
            result.collection_status,
            Some(UsageCollectionStatus::SignedOut)
        ));
        assert!(result.windows.is_empty());
        assert_eq!(transport.sent.len(), 3);
    }
    #[test]
    fn account_switch_discards_collected_quota() {
        let mut transport = fixture();
        transport.messages[3] = Ok(json!({"id":3,"result":{"account":null}}));
        assert!(collect_protocol(&mut transport, Instant::now() + DEADLINE).is_err());
    }
    #[test]
    fn unknown_percentage_is_not_zero() {
        let mut windows = vec![];
        append_rate_windows(
            &mut windows,
            &json!({"primary":{},"secondary":{"usedPercent":"invalid"}}),
        );
        assert!(windows.is_empty());
    }
    #[test]
    fn malformed_oversized_and_closed_streams_fail() {
        for data in [
            b"bad json\n".to_vec(),
            vec![b'x'; MAX_LINE_BYTES as usize + 1],
            vec![],
        ] {
            assert!(read_message(&mut std::io::Cursor::new(data)).is_err());
        }
    }
    #[test]
    fn write_failure_and_deadline_fail_closed() {
        let mut transport = fixture();
        transport.fail_write = true;
        assert!(collect_protocol(&mut transport, Instant::now() + DEADLINE).is_err());
        let mut transport = fixture();
        assert_eq!(
            collect_protocol(&mut transport, Instant::now()).unwrap_err(),
            ProviderError::Timeout
        );
    }
    #[test]
    fn duplicate_ids_cannot_complete_a_different_request_and_notifications_are_bounded() {
        let mut transport = fixture();
        transport.messages =
            std::iter::repeat_n(Ok(json!({"id":0,"result":{}})), MAX_MESSAGES + 2).collect();
        assert!(collect_protocol(&mut transport, Instant::now() + DEADLINE).is_err());
    }
}
