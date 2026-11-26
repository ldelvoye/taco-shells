use crate::flags::{self, Flags};
use crate::stages::{Measured, MemoryPoint};

const MEMORY_DECIMALS: usize = 0;

struct Summary {
    median: f64,
    ninety_fifth: f64,
    worst: f64,
}

pub fn render(flags: &Flags, measured: &Measured) -> String {
    let used = flags::describe(flags);
    let mut report = String::new();
    report.push_str(&format!(
        "Measured with {used}. The printing and filled terminals are the ones after the first in the list. \
Each delay ends when the change is in the window's view of the terminal, not when a frame shows it, \
because the driver has no frame timing yet. \
The Electron report's frame gap, long task, timer lag, output rate and graphics context rows \
have no equivalent here.\n\n"
    ));
    report.push_str("| Measure | Value |\n");
    report.push_str("| --- | --- |\n");

    for point in &measured.memory {
        for (name, value) in memory_values(point) {
            let row = format!("Memory, {}: {name}", point.label);
            push_row(&mut report, &row, &format!("{value:.MEMORY_DECIMALS$}"));
        }
    }

    let delays = [
        ("Typing delay, nothing printing", &measured.typing_quiet),
        ("Typing delay, while printing", &measured.typing_flooded),
        ("Switch, every entry in turn", &measured.switch_all),
        ("Switch, between two entries", &measured.switch_pair),
    ];
    for (name, samples) in delays {
        let label = format!("{name}: median / 95th / worst (ms)");
        let summary = summarize(samples);
        push_row(&mut report, &label, &spread(&summary));
    }
    report
}

fn memory_values(point: &MemoryPoint) -> [(&'static str, f64); 3] {
    [
        ("app", point.app),
        ("runtime", point.runtime),
        ("total", point.total()),
    ]
}

fn push_row(report: &mut String, label: &str, value: &str) {
    report.push_str(&format!("| {label} | {value} |\n"));
}

fn spread(summary: &Summary) -> String {
    format!(
        "{:.1} / {:.1} / {:.1}",
        summary.median, summary.ninety_fifth, summary.worst
    )
}

fn nearest_rank(ascending: &[f64], percentile: usize) -> f64 {
    let position = (percentile * ascending.len()).div_ceil(100);
    ascending[position - 1]
}

fn summarize(samples: &[f64]) -> Summary {
    if samples.is_empty() {
        return Summary {
            median: 0.0,
            ninety_fifth: 0.0,
            worst: 0.0,
        };
    }
    let mut ascending = samples.to_vec();
    ascending.sort_by(|a, b| a.total_cmp(b));
    let median = nearest_rank(&ascending, 50);
    let ninety_fifth = nearest_rank(&ascending, 95);
    let worst = ascending[ascending.len() - 1];
    Summary {
        median,
        ninety_fifth,
        worst,
    }
}
