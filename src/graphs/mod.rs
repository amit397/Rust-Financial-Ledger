use crate::eval::AccuracyReport;
use crate::stress::report::StressTestReport;

const COLORS: &[&str] = &[
    "#2563eb", "#dc2626", "#16a34a", "#ca8a04", "#9333ea", "#0891b2", "#e11d48", "#65a30d",
];

fn svg_rect(x: u32, y: u32, w: u32, h: u32, fill: &str, rx: u32) -> String {
    format!(
        r#"<rect x="{}" y="{}" width="{}" height="{}" rx="{}" fill="{}"/>"#,
        x, y, w, h, rx, fill
    )
}

fn svg_text(x: u32, y: u32, size: u32, fill: &str, anchor: &str, text: &str) -> String {
    format!(
        r#"<text x="{}" y="{}" font-size="{}" fill="{}" text-anchor="{}">{}</text>"#,
        x, y, size, fill, anchor, text
    )
}

fn svg_text_dominant(
    x: u32,
    y: u32,
    size: u32,
    fill: &str,
    anchor: &str,
    baseline: &str,
    text: &str,
) -> String {
    format!(
        r#"<text x="{}" y="{}" font-size="{}" fill="{}" text-anchor="{}" dominant-baseline="{}">{}</text>"#,
        x, y, size, fill, anchor, baseline, text
    )
}

fn svg_open(w: u32, h: u32) -> String {
    format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {} {}" width="{}" height="{}" style="font-family: system-ui, sans-serif; background: white;">"#,
        w, h, w, h
    )
}

fn svg_line(x1: u32, y1: u32, x2: u32, y2: u32, stroke: &str) -> String {
    format!(
        r#"<line x1="{}" y1="{}" x2="{}" y2="{}" stroke="{}" stroke-width="1"/>"#,
        x1, y1, x2, y2, stroke
    )
}

/// Generate SVG bar chart of rejection breakdown from stress test.
pub fn rejection_breakdown_svg(report: &StressTestReport) -> String {
    let categories: Vec<(&str, u64)> = vec![
        ("Insufficient Funds", report.rejected_insufficient),
        ("Account Not Found", report.rejected_not_found),
        ("Unbalanced", report.rejected_unbalanced),
        ("Overflow", report.rejected_overflow),
        ("Construction", report.rejected_construction),
        ("Parse Failure", report.rejected_parse),
    ];

    let max_val = categories.iter().map(|(_, v)| *v).max().unwrap_or(1).max(1);
    let bar_height: u32 = 32;
    let gap: u32 = 8;
    let label_width: u32 = 160;
    let chart_width: u32 = 400;
    let total_width = label_width + chart_width + 80;
    let total_height = categories.len() as u32 * (bar_height + gap) + 60;

    let mut svg = svg_open(total_width, total_height);
    svg.push_str(&svg_text(
        total_width / 2,
        24,
        16,
        "#1e293b",
        "middle",
        "Rejection Breakdown",
    ));

    for (i, (label, value)) in categories.iter().enumerate() {
        let y = 44 + i as u32 * (bar_height + gap);
        let bar_w = if max_val > 0 {
            (*value as f64 / max_val as f64 * chart_width as f64) as u32
        } else {
            0
        };
        let color = COLORS[i % COLORS.len()];

        svg.push_str(&svg_text_dominant(
            label_width - 8,
            y + bar_height / 2,
            12,
            "#475569",
            "end",
            "middle",
            label,
        ));
        svg.push_str(&svg_rect(label_width, y, bar_w, bar_height, color, 4));
        svg.push_str(&svg_text_dominant(
            label_width + bar_w + 6,
            y + bar_height / 2,
            11,
            "#1e293b",
            "start",
            "middle",
            &value.to_string(),
        ));
    }

    svg.push_str("</svg>");
    svg
}

/// Generate SVG bar chart of per-agent commit rates from stress test.
pub fn per_agent_commit_svg(report: &StressTestReport) -> String {
    let bar_width: u32 = 50;
    let gap: u32 = 20;
    let chart_height: u32 = 250;
    let margin_left: u32 = 60;
    let margin_top: u32 = 40;
    let margin_bottom: u32 = 80;
    let total_width =
        margin_left + report.per_agent.len() as u32 * (bar_width * 2 + gap) + 40;
    let total_height = margin_top + chart_height + margin_bottom;

    let max_val = report
        .per_agent
        .iter()
        .map(|a| a.proposed)
        .max()
        .unwrap_or(1)
        .max(1);

    let mut svg = svg_open(total_width, total_height);
    svg.push_str(&svg_text(
        total_width / 2,
        24,
        16,
        "#1e293b",
        "middle",
        "Per-Agent: Proposed vs Committed",
    ));

    svg.push_str(&svg_line(
        margin_left,
        margin_top,
        margin_left,
        margin_top + chart_height,
        "#cbd5e1",
    ));

    let base_y = margin_top + chart_height;

    for (i, agent) in report.per_agent.iter().enumerate() {
        let x = margin_left + i as u32 * (bar_width * 2 + gap) + 10;
        let proposed_h =
            (agent.proposed as f64 / max_val as f64 * chart_height as f64) as u32;
        let committed_h =
            (agent.committed as f64 / max_val as f64 * chart_height as f64) as u32;

        svg.push_str(&svg_rect(
            x,
            base_y - proposed_h,
            bar_width,
            proposed_h,
            "#93c5fd",
            3,
        ));
        svg.push_str(&svg_rect(
            x + bar_width,
            base_y - committed_h,
            bar_width,
            committed_h,
            "#2563eb",
            3,
        ));

        let label = if agent.name.len() > 12 {
            &agent.name[..12]
        } else {
            &agent.name
        };
        svg.push_str(&format!(
            "<text x=\"{}\" y=\"{}\" font-size=\"10\" fill=\"#475569\" text-anchor=\"middle\" transform=\"rotate(-30, {}, {})\">{}</text>",
            x + bar_width, base_y + 20, x + bar_width, base_y + 20, label
        ));

        let rate = if agent.proposed > 0 {
            agent.committed as f64 / agent.proposed as f64 * 100.0
        } else {
            0.0
        };
        svg.push_str(&svg_text(
            x + bar_width,
            base_y - committed_h.max(proposed_h) - 4,
            9,
            "#1e293b",
            "middle",
            &format!("{:.0}%", rate),
        ));
    }

    // Legend
    let ly = total_height - 20;
    svg.push_str(&svg_rect(margin_left, ly, 14, 14, "#93c5fd", 2));
    svg.push_str(&svg_text(margin_left + 18, ly + 12, 11, "#475569", "start", "Proposed"));
    svg.push_str(&svg_rect(margin_left + 90, ly, 14, 14, "#2563eb", 2));
    svg.push_str(&svg_text(
        margin_left + 108,
        ly + 12,
        11,
        "#475569",
        "start",
        "Committed",
    ));

    svg.push_str("</svg>");
    svg
}

/// Generate SVG bar chart of eval accuracy by category.
pub fn eval_accuracy_svg(report: &AccuracyReport) -> String {
    let mut categories: Vec<&String> = report.by_category.keys().collect();
    categories.sort();

    let bar_width: u32 = 60;
    let gap: u32 = 30;
    let chart_height: u32 = 220;
    let margin_left: u32 = 60;
    let margin_top: u32 = 50;
    let margin_bottom: u32 = 60;
    let total_width =
        margin_left + categories.len() as u32 * (bar_width * 2 + gap) + 40;
    let total_height = margin_top + chart_height + margin_bottom;

    let mut svg = svg_open(total_width, total_height);
    svg.push_str(&svg_text(
        total_width / 2,
        28,
        16,
        "#1e293b",
        "middle",
        "Eval: Parse Rate vs Commit Rate by Category",
    ));

    // Grid lines at 25%, 50%, 75%, 100%
    for pct in &[25u32, 50, 75, 100] {
        let y =
            margin_top + chart_height - (*pct as f64 / 100.0 * chart_height as f64) as u32;
        svg.push_str(&svg_line(margin_left, y, total_width - 20, y, "#e2e8f0"));
        svg.push_str(&svg_text(
            margin_left - 6,
            y + 4,
            10,
            "#94a3b8",
            "end",
            &format!("{}%", pct),
        ));
    }

    let base_y = margin_top + chart_height;

    for (i, cat) in categories.iter().enumerate() {
        let c = &report.by_category[*cat];
        let parse_pct = if c.total > 0 {
            c.parse_success as f64 / c.total as f64
        } else {
            0.0
        };
        let commit_pct = if c.total > 0 {
            c.ledger_committed as f64 / c.total as f64
        } else {
            0.0
        };

        let x = margin_left + i as u32 * (bar_width * 2 + gap) + 10;
        let parse_h = (parse_pct * chart_height as f64) as u32;
        let commit_h = (commit_pct * chart_height as f64) as u32;

        svg.push_str(&svg_rect(x, base_y - parse_h, bar_width, parse_h, "#86efac", 3));
        svg.push_str(&svg_text(
            x + bar_width / 2,
            base_y - parse_h - 4,
            10,
            "#1e293b",
            "middle",
            &format!("{:.0}%", parse_pct * 100.0),
        ));

        svg.push_str(&svg_rect(
            x + bar_width,
            base_y - commit_h,
            bar_width,
            commit_h,
            "#16a34a",
            3,
        ));
        svg.push_str(&svg_text(
            x + bar_width + bar_width / 2,
            base_y - commit_h - 4,
            10,
            "#1e293b",
            "middle",
            &format!("{:.0}%", commit_pct * 100.0),
        ));

        svg.push_str(&svg_text(
            x + bar_width,
            base_y + 18,
            11,
            "#475569",
            "middle",
            cat,
        ));
    }

    // Legend
    let ly = total_height - 16;
    svg.push_str(&svg_rect(margin_left, ly, 14, 14, "#86efac", 2));
    svg.push_str(&svg_text(
        margin_left + 18,
        ly + 12,
        11,
        "#475569",
        "start",
        "Parse Rate",
    ));
    svg.push_str(&svg_rect(margin_left + 110, ly, 14, 14, "#16a34a", 2));
    svg.push_str(&svg_text(
        margin_left + 128,
        ly + 12,
        11,
        "#475569",
        "start",
        "Commit Rate",
    ));

    svg.push_str("</svg>");
    svg
}

/// Generate SVG latency histogram from sampled durations.
pub fn latency_histogram_svg(latencies_ms: &[f64]) -> String {
    if latencies_ms.is_empty() {
        return format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="400" height="100">{}</svg>"#,
            svg_text(200, 50, 14, "#94a3b8", "middle", "No latency data")
        );
    }

    let mut sorted = latencies_ms.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());

    let min_val = sorted[0];
    let max_val = sorted[sorted.len() - 1];
    let num_buckets = 20usize;
    let bucket_width_val = if max_val > min_val {
        (max_val - min_val) / num_buckets as f64
    } else {
        1.0
    };

    let mut buckets = vec![0u32; num_buckets];
    for &v in &sorted {
        let idx = ((v - min_val) / bucket_width_val) as usize;
        buckets[idx.min(num_buckets - 1)] += 1;
    }

    let max_count = *buckets.iter().max().unwrap_or(&1).max(&1);
    let bar_w: u32 = 24;
    let chart_height: u32 = 200;
    let margin_left: u32 = 50;
    let margin_top: u32 = 50;
    let margin_bottom: u32 = 50;
    let total_width = margin_left + num_buckets as u32 * bar_w + 40;
    let total_height = margin_top + chart_height + margin_bottom;

    let mut svg = svg_open(total_width, total_height);
    svg.push_str(&svg_text(
        total_width / 2,
        28,
        16,
        "#1e293b",
        "middle",
        "Latency Distribution (ms)",
    ));

    let base_y = margin_top + chart_height;

    for (i, &count) in buckets.iter().enumerate() {
        let h = (count as f64 / max_count as f64 * chart_height as f64) as u32;
        let x = margin_left + i as u32 * bar_w;
        svg.push_str(&svg_rect(x, base_y - h, bar_w - 2, h, "#6366f1", 1));
    }

    // X-axis labels (every 5th bucket)
    for i in (0..num_buckets).step_by(5) {
        let val = min_val + i as f64 * bucket_width_val;
        let x = margin_left + i as u32 * bar_w;
        svg.push_str(&svg_text(
            x + bar_w / 2,
            base_y + 14,
            9,
            "#475569",
            "middle",
            &format!("{:.2}", val),
        ));
    }

    // P50/P95 markers
    let p50_idx = (sorted.len() as f64 * 0.5) as usize;
    let p95_idx = ((sorted.len() as f64 * 0.95) as usize).min(sorted.len() - 1);
    let p50_val = sorted[p50_idx];
    let p95_val = sorted[p95_idx];

    svg.push_str(&svg_text(
        margin_left,
        base_y + 36,
        10,
        "#1e293b",
        "start",
        &format!(
            "p50: {:.3}ms  p95: {:.3}ms  n={}",
            p50_val,
            p95_val,
            sorted.len()
        ),
    ));

    svg.push_str("</svg>");
    svg
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stress::report::{PerAgentReport, StressTestReport};

    fn mock_report() -> StressTestReport {
        StressTestReport {
            duration_secs: 10.0,
            agent_count: 2,
            agent_names: vec!["A".into(), "B".into()],
            total_proposals: 1000,
            total_committed: 500,
            total_rejected: 500,
            rejected_insufficient: 200,
            rejected_not_found: 100,
            rejected_unbalanced: 80,
            rejected_overflow: 50,
            rejected_construction: 40,
            rejected_parse: 30,
            total_panics: 0,
            throughput_per_sec: 100.0,
            p50_latency_ms: Some(0.05),
            p95_latency_ms: Some(0.1),
            p99_latency_ms: Some(0.2),
            contention_pct: 5.0,
            per_agent: vec![
                PerAgentReport {
                    name: "A".into(),
                    proposed: 500,
                    committed: 400,
                    rejected: 100,
                },
                PerAgentReport {
                    name: "B".into(),
                    proposed: 500,
                    committed: 100,
                    rejected: 400,
                },
            ],
        }
    }

    #[test]
    fn test_rejection_svg_valid() {
        let svg = rejection_breakdown_svg(&mock_report());
        assert!(svg.starts_with("<svg"));
        assert!(svg.ends_with("</svg>"));
        assert!(svg.contains("Insufficient Funds"));
    }

    #[test]
    fn test_agent_commit_svg_valid() {
        let svg = per_agent_commit_svg(&mock_report());
        assert!(svg.starts_with("<svg"));
        assert!(svg.contains("Proposed"));
    }

    #[test]
    fn test_latency_histogram_empty() {
        let svg = latency_histogram_svg(&[]);
        assert!(svg.contains("No latency data"));
    }

    #[test]
    fn test_latency_histogram_with_data() {
        let data: Vec<f64> = (0..100).map(|i| i as f64 * 0.01).collect();
        let svg = latency_histogram_svg(&data);
        assert!(svg.contains("p50"));
        assert!(svg.contains("<rect"));
    }
}
