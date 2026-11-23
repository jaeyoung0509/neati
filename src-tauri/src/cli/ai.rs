//! `neati ai`: the AI tool usage the app shows, derived data only.
//!
//! Provider secrets stay where the app keeps them; the snapshot carries
//! percentages, token counts and status text, never a credential.

use std::sync::Arc;

use serde::Serialize;

use crate::models::{AiUsageSnapshot, UsageSupport};
use crate::services::insights::InsightQueries;
use crate::services::ports::{Activity, ActivityPort};

use super::output::{print_json, Progress};

#[derive(Serialize)]
struct AiReport {
    #[serde(flatten)]
    snapshot: AiUsageSnapshot,
}

pub fn ai(
    insights: &dyn InsightQueries,
    progress: Arc<Progress>,
    refresh: bool,
    json: bool,
) -> Result<i32, String> {
    progress.report(Activity::Checking {
        source: "AI tools".to_string(),
    });
    let snapshot = tauri::async_runtime::block_on(insights.ai_usage(refresh, progress.clone()));
    progress.clear();
    let snapshot = snapshot?;
    if json {
        print_json("ai", AiReport { snapshot })?;
        return Ok(0);
    }
    let shown: Vec<_> = snapshot
        .providers
        .iter()
        .filter(|provider| provider.installed || provider.connected)
        .collect();
    if shown.is_empty() {
        println!("No supported AI tool was found. Configure providers in neati (AI → Accounts).");
        return Ok(0);
    }
    for provider in shown {
        println!("{} · {}", provider.name, support_label(&provider.support));
        if provider.windows.is_empty() && !provider.status_message.is_empty() {
            println!("  {}", provider.status_message);
        }
        for window in &provider.windows {
            println!("  {:<18} {:>5.1}% used", window.label, window.used_percent);
        }
        if let Some(tokens) = provider.summary.last_7d_tokens {
            println!("  {:<18} {tokens} tokens", "last 7 days");
        }
        if let Some(cost) = provider.summary.local_cost_usd {
            println!("  {:<18} ${cost:.2} (local estimate)", "spend");
        }
    }
    Ok(0)
}

fn support_label(support: &UsageSupport) -> &'static str {
    match support {
        UsageSupport::Live => "live",
        UsageSupport::Local => "local logs",
        UsageSupport::Manual => "manual",
    }
}
