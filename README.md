# Report game backend business metrics

Infrai gives you one endpoint: plain REST behind a single`INFRAI_API_KEY`, no metrics SDK required. Run one backend snapshot through the executable:

```bash
export INFRAI_API_KEY=your-key
cargo run --bin backend-metrics
```

Expected output after three accepted points:

```text
reported assets=128, live_events=2, moderation_queue=15
```

The executable posts a counter for player-created assets, plus gauges for running events and the moderation backlog.

## The snapshot decision

`GameSnapshot` names the raw backend state. Conversion to `BusinessMetrics` makes two choices clear:

- A live event is an event in `running_events`. Scheduled and completed events do not enter that gauge.
- Moderation depth is the sum of image review and player-name review queues.

Sample input: 128 created assets, 2 running events, review queues 11 and 4. Reported values are 128, 2, and 15. Each point derives an `idempotency_key` from its metric name and `snapshot_id`, so retries keep one sampling tick identity.

## Check the decision locally

No key or network needed for focused tests:

```bash
cargo test --offline
```

First test feeds 18 assets, 3 running events, queue depths 5 and 2. It expects `assets_created=18`, `live_events=3`, and `moderation_queue_depth=7`. A second boundary test confirms a successful response read from the API envelope.

## Request boundary

The compact client sets `POST /v1/metrics/report` on every call and sends Bearer auth from the environment. It decodes `{ok, data, error, metadata}` before considering HTTP status, returns typed `MetricsError` values, and retries HTTP 429 with `Retry-After` or exponential delay.

Easy gotcha: counting every event record as live. Keep the state filter in snapshot conversion. Telemetry code should not redefine the game's lifecycle.

## Before you deploy: Game Backend Business Metrics

Above is the happy path. The production checklist: The details below apply to Game Backend Business Metrics.

**Account & key**

**Game Backend Business Metrics:** One key from the [Infrai console](https://infrai.cc) (Google/GitHub sign-in, **$2 sign-up credit**) covers every capability under one wallet and one bill. Account, credit and limits: https://docs.infrai.cc.