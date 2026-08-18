mod game_metrics;

use game_metrics::{block_on, GameSnapshot, InfraiMetrics, MetricsError};

fn main() -> Result<(), MetricsError> {
    block_on(async {
        let client = InfraiMetrics::from_env()?;
        let reported = client.report_game_snapshot(
            "eu-2",
            "tick-2026-08-17T10:15:00Z",
            GameSnapshot {
                player_assets_created: 128,
                scheduled_events: 6,
                running_events: 2,
                completed_events: 39,
                image_review_queue: 11,
                name_review_queue: 4,
            },
        ).await?;
        println!(
            "reported assets={}, live_events={}, moderation_queue={}",
            reported.assets_created, reported.live_events, reported.moderation_queue_depth
        );
        Ok(())
    })
}

