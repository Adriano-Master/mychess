//! Progress as plain lines, one per 10%: no carriage returns, so it works
//! the same in a terminal, a pipe or a log.

use std::io::Write;

use mychess_db::ProgressSink;

/// Writes `label: NN%` to `err` each time the percentage passes a multiple of 10.
pub(crate) struct Reporter<'a> {
    err: &'a mut dyn Write,
    label: &'static str,
    enabled: bool,
    last_step: u64,
}

impl<'a> Reporter<'a> {
    pub(crate) fn new(err: &'a mut dyn Write, label: &'static str, quiet: bool) -> Reporter<'a> {
        Reporter {
            err,
            label,
            enabled: !quiet,
            last_step: 0,
        }
    }
}

impl ProgressSink for Reporter<'_> {
    fn progress(&mut self, done: u64, total: u64) {
        if !self.enabled || total == 0 {
            return;
        }
        let percent =
            u64::try_from(u128::from(done.min(total)) * 100 / u128::from(total)).unwrap_or(100);
        let step = percent / 10;
        if step > self.last_step {
            self.last_step = step;
            writeln!(self.err, "{}: {}%", self.label, step * 10).ok();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_each_ten_percent_once() {
        let mut buffer = Vec::new();
        {
            let mut reporter = Reporter::new(&mut buffer, "import", false);
            for done in [0, 5, 10, 12, 35, 35, 100] {
                reporter.progress(done, 100);
            }
        }
        assert_eq!(
            String::from_utf8(buffer).unwrap(),
            "import: 10%\nimport: 30%\nimport: 100%\n"
        );
    }

    #[test]
    fn quiet_and_unknown_total_print_nothing() {
        let mut buffer = Vec::new();
        {
            let mut quiet = Reporter::new(&mut buffer, "x", true);
            quiet.progress(50, 100);
            let mut unknown = Reporter::new(&mut buffer, "x", false);
            unknown.progress(50, 0);
        }
        assert_eq!(buffer.len(), 0);
    }
}
