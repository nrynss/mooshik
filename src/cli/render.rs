//! Turning memory results into terminal output, in one voice.

use lambo::{ConceptType, MemoryStats, RecallResult};

use crate::text;

/// Render recall results. Local-operator output only — see
/// `memory::ops::recall` for why this path deliberately skips chat's egress
/// redaction: nothing recalled here reaches a model or history.
pub(crate) fn render_recall(query: &str, recalled: &RecallResult) -> String {
    if recalled.hits.is_empty() {
        return text::get("memory.recall_empty").replace("{query}", query);
    }
    let mut out = text::get("memory.recall_header").replace("{query}", query);
    out.push('\n');
    for (index, hit) in recalled.hits.iter().enumerate() {
        out.push_str(&format!("\n  {}. {}\n     ", index + 1, hit.content));
        let mut detail: Vec<String> = Vec::new();
        if let Some(kind) = hit.concept_type {
            detail.push(concept_kind(kind).to_owned());
        }
        if hit.is_canonical {
            detail.push(text::get("memory.recall_canonical").to_owned());
        }
        detail.push(
            text::get("memory.recall_relevance").replace("{score}", &format!("{:.2}", hit.score)),
        );
        if let Some(radius) = hit.blast_radius {
            detail.push(
                text::get("memory.recall_blast_radius").replace("{count}", &radius.to_string()),
            );
        }
        out.push_str(&detail.join(" · "));
        out.push('\n');
    }
    if !recalled.warnings.is_empty() {
        out.push('\n');
        out.push_str(text::get("memory.recall_warnings"));
        out.push('\n');
        for warning in &recalled.warnings {
            out.push_str(&format!("  - {warning}\n"));
        }
    }
    out
}

fn concept_kind(kind: ConceptType) -> &'static str {
    match kind {
        ConceptType::Entity => text::get("memory.kind_entity"),
        ConceptType::Logic => text::get("memory.kind_logic"),
        ConceptType::Constraint => text::get("memory.kind_constraint"),
        ConceptType::Resource => text::get("memory.kind_resource"),
        ConceptType::Observation => text::get("memory.kind_observation"),
    }
}

pub(crate) fn render_stats(health: &MemoryStats) -> String {
    let degraded = if health.degraded {
        text::get("memory.degraded_yes")
    } else {
        text::get("memory.degraded_no")
    };
    [
        text::get("memory.stats_header")
            .replace("{session}", health.session.as_str())
            .replace("{agent}", health.agent.as_str()),
        format!(
            "  {}",
            text::get("memory.stats_concepts")
                .replace("{total}", &health.concept_count.to_string())
                .replace("{canonical}", &health.canonical_count.to_string())
                .replace("{embedded}", &health.embedded_concepts.to_string())
        ),
        format!(
            "  {}",
            text::get("memory.stats_graph")
                .replace("{nodes}", &health.node_count.to_string())
                .replace("{edges}", &health.edge_count.to_string())
        ),
        format!(
            "  {}",
            text::get("memory.stats_log_depth").replace("{depth}", &health.log_depth.to_string())
        ),
        format!(
            "  {}",
            text::get("memory.stats_flush_lag")
                .replace("{lag}", &format!("{:.1}s", health.flush_lag.as_secs_f64()),)
        ),
        format!(
            "  {}",
            text::get("memory.stats_dead_letters")
                .replace("{count}", &health.dead_lettered.to_string())
        ),
        format!(
            "  {}",
            text::get("memory.stats_degraded").replace("{degraded}", degraded)
        ),
        format!(
            "  {}",
            text::get("memory.stats_cycles")
                .replace("{daemon}", &health.daemon_cycles.to_string())
                .replace("{canonization}", &health.canonization_cycles.to_string())
                .replace("{failures}", &health.canonization_failures.to_string())
        ),
    ]
    .join("\n")
}
