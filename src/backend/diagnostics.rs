use std::time::Duration;

#[derive(Default)]
pub(super) struct Diagnostics {
    failure: Option<String>,
    last_report: Duration,
    suppressed: u64,
}

impl Diagnostics {
    pub(super) fn record(
        &mut self,
        now: Duration,
        failure: Option<String>,
        verbose: bool,
    ) -> Option<String> {
        let Some(failure) = failure else {
            self.failure.take()?;
            let count = std::mem::take(&mut self.suppressed);
            return Some(format!(
                "playback recovered ({count} repeated failures suppressed)"
            ));
        };
        let changed = self.failure.as_ref() != Some(&failure);
        if !changed && !verbose && now.saturating_sub(self.last_report) < Duration::from_secs(60) {
            self.suppressed = self.suppressed.saturating_add(1);
            return None;
        }
        let count = std::mem::take(&mut self.suppressed);
        self.last_report = now;
        self.failure = Some(failure.clone());
        Some(format!("{failure} ({count} repeated failures suppressed)"))
    }
}
