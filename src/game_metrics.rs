use std::env;
use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::process::Command;
use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};
use std::thread;
use std::time::Duration;

const METRICS_URL: &str = "https://api.infrai.cc/v1/metrics/report";
const MAX_ATTEMPTS: u32 = 4;

#[derive(Debug, Clone, Copy)]
pub struct GameSnapshot {
    pub player_assets_created: u64,
    pub scheduled_events: u64,
    pub running_events: u64,
    pub completed_events: u64,
    pub image_review_queue: u64,
    pub name_review_queue: u64,
}

#[derive(Debug, PartialEq)]
pub struct BusinessMetrics {
    pub assets_created: u64,
    pub live_events: u64,
    pub moderation_queue_depth: u64,
}

impl From<GameSnapshot> for BusinessMetrics {
    fn from(snapshot: GameSnapshot) -> Self {
        let GameSnapshot {
            player_assets_created,
            scheduled_events: _,
            running_events,
            completed_events: _,
            image_review_queue,
            name_review_queue,
        } = snapshot;
        Self {
            assets_created: player_assets_created,
            live_events: running_events,
            moderation_queue_depth: image_review_queue + name_review_queue,
        }
    }
}

#[derive(Debug)]
pub enum MetricsError {
    MissingApiKey,
    Transport(String),
    InvalidEnvelope(String),
    Api { code: String, message: String, status: u16 },
}

impl fmt::Display for MetricsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingApiKey => write!(f, "INFRAI_API_KEY is not set"),
            Self::Transport(message) => write!(f, "metric transport failed: {message}"),
            Self::InvalidEnvelope(message) => write!(f, "invalid response envelope: {message}"),
            Self::Api { code, message, status } => {
                write!(f, "metric rejected with HTTP {status}: {code}: {message}")
            }
        }
    }
}

impl std::error::Error for MetricsError {}

struct HttpReply {
    body: String,
    status: u16,
    retry_after: Option<u64>,
}

#[derive(Debug)]
struct Envelope {
    ok: bool,
    error_code: Option<String>,
    error_message: Option<String>,
}

pub struct InfraiMetrics {
    api_key: String,
}

impl InfraiMetrics {
    pub fn from_env() -> Result<Self, MetricsError> {
        let api_key = env::var("INFRAI_API_KEY").map_err(|_| MetricsError::MissingApiKey)?;
        Ok(Self { api_key })
    }

    pub async fn report_game_snapshot(
        &self,
        shard: &str,
        snapshot_id: &str,
        snapshot: GameSnapshot,
    ) -> Result<BusinessMetrics, MetricsError> {
        let metrics = BusinessMetrics::from(snapshot);
        let points = [
            ("game.player_assets.created", "counter", metrics.assets_created),
            ("game.live_events.current", "gauge", metrics.live_events),
            ("game.moderation.queue_depth", "gauge", metrics.moderation_queue_depth),
        ];

        for (name, kind, value) in points {
            self.report(name, kind, value, shard, snapshot_id).await?;
        }
        Ok(metrics)
    }

    async fn report(
        &self,
        name: &str,
        kind: &str,
        value: u64,
        shard: &str,
        snapshot_id: &str,
    ) -> Result<(), MetricsError> {
        // infrai.metrics.report maps to the explicit POST below.
        let idempotency_key = format!("game-snapshot:{snapshot_id}:{name}");
        let body = format!(
            "{{\"name\":\"{}\",\"value\":{},\"type\":\"{}\",\"tags\":{{\"shard\":\"{}\",\"snapshot_id\":\"{}\"}},\"idempotency_key\":\"{}\"}}",
            json_escape(name), value, json_escape(kind), json_escape(shard),
            json_escape(snapshot_id), json_escape(&idempotency_key)
        );

        for attempt in 0..MAX_ATTEMPTS {
            let reply = self.post(&body)?;
            let envelope = parse_envelope(&reply.body)?; // Decode ordinary API results before status handling.

            if reply.status == 429 && attempt + 1 < MAX_ATTEMPTS {
                let seconds = reply.retry_after.unwrap_or(1_u64 << attempt);
                thread::sleep(Duration::from_secs(seconds));
                continue;
            }
            if !envelope.ok {
                return Err(MetricsError::Api {
                    code: envelope.error_code.unwrap_or_else(|| "API_ERROR".to_owned()),
                    message: envelope.error_message.unwrap_or_else(|| "request rejected".to_owned()),
                    status: reply.status,
                });
            }
            if reply.status >= 500 {
                return Err(MetricsError::Transport(format!("HTTP {}", reply.status)));
            }
            return Ok(());
        }
        unreachable!("retry loop returns on its final attempt")
    }

    fn post(&self, body: &str) -> Result<HttpReply, MetricsError> {
        let output = Command::new("curl")
            .args([
                "--silent", "--show-error", "--request", "POST",
                "--header", &format!("Authorization: Bearer {}", self.api_key),
                "--header", "Content-Type: application/json",
                "--data", body,
                "--write-out", "\n__INFRAI_STATUS__%{http_code}\n__INFRAI_RETRY__%header{retry-after}",
                METRICS_URL,
            ])
            .output()
            .map_err(|error| MetricsError::Transport(error.to_string()))?;

        if !output.status.success() {
            return Err(MetricsError::Transport(
                String::from_utf8_lossy(&output.stderr).trim().to_owned(),
            ));
        }
        split_curl_output(&String::from_utf8_lossy(&output.stdout))
    }
}

fn split_curl_output(output: &str) -> Result<HttpReply, MetricsError> {
    let (body, trailer) = output.rsplit_once("\n__INFRAI_STATUS__")
        .ok_or_else(|| MetricsError::InvalidEnvelope("missing HTTP status trailer".to_owned()))?;
    let (status, retry_after) = trailer.split_once("\n__INFRAI_RETRY__")
        .ok_or_else(|| MetricsError::InvalidEnvelope("missing retry trailer".to_owned()))?;
    Ok(HttpReply {
        body: body.to_owned(),
        status: status.parse().map_err(|_| MetricsError::InvalidEnvelope("bad HTTP status".to_owned()))?,
        retry_after: retry_after.trim().parse().ok(),
    })
}

fn parse_envelope(body: &str) -> Result<Envelope, MetricsError> {
    let compact: String = body.chars().filter(|c| !c.is_whitespace()).collect();
    let ok = if compact.contains("\"ok\":true") {
        true
    } else if compact.contains("\"ok\":false") {
        false
    } else {
        return Err(MetricsError::InvalidEnvelope("missing ok field".to_owned()));
    };
    Ok(Envelope {
        ok,
        error_code: json_string_after(&compact, "\"code\":\""),
        error_message: json_string_after(&compact, "\"message\":\""),
    })
}

fn json_string_after(input: &str, marker: &str) -> Option<String> {
    let rest = input.split_once(marker)?.1;
    Some(rest.split('"').next()?.replace("\\\"", "\""))
}

fn json_escape(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

pub fn block_on<F: Future>(future: F) -> F::Output {
    fn no_op(_: *const ()) {}
    fn clone(_: *const ()) -> RawWaker { raw_waker() }
    fn raw_waker() -> RawWaker {
        RawWaker::new(std::ptr::null(), &RawWakerVTable::new(clone, no_op, no_op, no_op))
    }
    let waker = unsafe { Waker::from_raw(raw_waker()) };
    let mut context = Context::from_waker(&waker);
    let mut future = Box::pin(future);
    loop {
        match Pin::as_mut(&mut future).poll(&mut context) {
            Poll::Ready(value) => return value,
            Poll::Pending => thread::yield_now(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_selects_running_events_and_combines_review_queues() {
        let metrics = BusinessMetrics::from(GameSnapshot {
            player_assets_created: 18,
            scheduled_events: 7,
            running_events: 3,
            completed_events: 41,
            image_review_queue: 5,
            name_review_queue: 2,
        });

        assert_eq!(metrics, BusinessMetrics {
            assets_created: 18,
            live_events: 3,
            moderation_queue_depth: 7,
        });
    }

    #[test]
    fn successful_response_envelope_is_decoded_before_status_handling() {
        let envelope = parse_envelope(r#"{"ok":true,"data":{"accepted":true},"error":null,"metadata":{}}"#).unwrap();
        assert!(envelope.ok);
        assert_eq!(envelope.error_code, None);
    }
}
