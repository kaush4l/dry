//! The display components (`dry_core::show::Show`), drawn like the
//! window's (`crates/gpui/src/views/show.rs`): a table, a list, a progress
//! bar, a row of stats. They only show, so they look the same live and
//! read-only.

use std::fmt::Write;

use dry_core::show::{Show, Trend};

use super::esc;

/// Draw `s`.
pub fn draw(out: &mut String, s: &Show) {
    let _ = write!(out, "<div class=\"glass panel show\" data-show=\"{}\">", s.name());
    if !s.caption().is_empty() {
        let _ = write!(out, "<div class=\"section strong\">{}</div>", esc(s.caption()));
    }
    match s {
        Show::Table { head, rows, .. } => {
            out.push_str("<div class=\"show-scroll\"><table class=\"show-table\"><thead><tr>");
            for h in head {
                let _ = write!(out, "<th>{}</th>", esc(h));
            }
            out.push_str("</tr></thead><tbody>");
            for r in rows {
                out.push_str("<tr>");
                for c in r {
                    let _ = write!(out, "<td>{}</td>", esc(c));
                }
                out.push_str("</tr>");
            }
            out.push_str("</tbody></table></div>");
        }
        Show::List { items, .. } => {
            out.push_str("<ul class=\"show-list\">");
            for i in items {
                let _ = write!(out, "<li>{}</li>", esc(i));
            }
            out.push_str("</ul>");
        }
        Show::Progress { .. } => {
            let pct = s.fraction() * 100.0;
            let _ = write!(
                out,
                "<div class=\"gauge\" role=\"progressbar\" aria-valuenow=\"{pct:.0}\" aria-valuemin=\"0\" aria-valuemax=\"100\"><i style=\"width:{pct:.1}%\"></i></div><div class=\"note\">{}</div>",
                esc(&s.progress_text())
            );
        }
        Show::Stat { stats, .. } => {
            out.push_str("<div class=\"stats\">");
            for st in stats {
                let trend = match st.trend() {
                    Trend::Up => "up",
                    Trend::Down => "down",
                    Trend::Flat => "flat",
                };
                let _ = write!(out, "<div class=\"stat\"><div class=\"note\">{}</div><div class=\"stat-v\">{}</div>", esc(&st.label), esc(&st.value));
                if !st.delta.is_empty() {
                    let _ = write!(out, "<div class=\"delta {trend}\">{}</div>", esc(&st.delta));
                }
                out.push_str("</div>");
            }
            out.push_str("</div>");
        }
    }
    out.push_str("</div>");
}
