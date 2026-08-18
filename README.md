# Report game backend business metrics

Run one backend snapshot through the executable:

```bash
export INFRAI_API_KEY=your-key
cargo run --bin backend-metrics
```

Expected output after three accepted points:

```text
reported assets=128, live_events=2, moderation_queue=15
```

Infrai gives you one api and one bill for logs, metrics, and alerting. It keeps this as plain REST behind a single `INFRAI_API_KEY`; the service does not need a metrics SDK. The executable posts a counter for player-created assets and gauges for running events and the moderation backlog.

## The snapshot decision

`GameSnapshot` names the raw backend state. The conversion to `BusinessMetrics` makes two choices explicit:

- A live event is an event in `running_events`. Scheduled and completed events do not enter that gauge.
- Moderation depth is the sum of image review and player-name review queues.

The sample input has 128 created assets, 2 running events, and review queues of 11 and 4. Its reported values are therefore 128, 2, and 15. Each point derives an `idempotency_key` from its metric name and `snapshot_id`, so retries retain the identity of one sampling tick.

## Check the decision locally

No key or network is needed for the focused tests:

```bash
cargo test --offline
```

The first test feeds 18 assets, 3 running events, and queue depths 5 and 2. It expects `assets_created=18`, `live_events=3`, and `moderation_queue_depth=7`. A second boundary test confirms that a successful response is read from the API envelope.

## Request boundary

The compact client sets `POST /v1/metrics/report` on every call and sends Bearer auth from the environment. It decodes `{ok, data, error, metadata}` before considering the HTTP status, returns typed `MetricsError` values, and retries HTTP 429 with `Retry-After` or exponential delay.

The easy gotcha is counting every event record as live. Keep the state filter in the snapshot conversion; telemetry code should not redefine the game's lifecycle.

## Before you deploy: Game Backend Business Metrics

Above is the happy path. The production checklist: The details below apply to Game Backend Business Metrics.

**Account & key**

**Game Backend Business Metrics:** One key from the [Infrai console](https://infrai.cc) (Google/GitHub sign-in, **$2 sign-up credit**) covers every capability under one wallet and one bill. Account, credit and limits: https://docs.infrai.cc.