use super::*;

fn canonical_run(jsonl: &str) -> Option<String> {
    jsonl.lines().find_map(|line| {
        serde_json::from_str::<serde_json::Value>(line)
            .ok()?
            .get("run")?
            .as_str()
            .map(ToOwned::to_owned)
    })
}

mod durable;
mod frames;
mod listener;
