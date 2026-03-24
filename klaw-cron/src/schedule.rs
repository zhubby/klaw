use crate::{time::ms_to_utc, CronError};
use klaw_storage::{CronJob, CronScheduleKind};
use std::{str::FromStr, time::Duration};

#[derive(Debug, Clone)]
pub enum ScheduleSpec {
    Cron(cron::Schedule),
    Every(Duration),
}

impl ScheduleSpec {
    pub fn from_kind_expr(kind: CronScheduleKind, expr: &str) -> Result<Self, CronError> {
        match kind {
            CronScheduleKind::Cron => {
                let schedule = cron::Schedule::from_str(expr)
                    .map_err(|err| CronError::InvalidSchedule(err.to_string()))?;
                Ok(Self::Cron(schedule))
            }
            CronScheduleKind::Every => {
                let parsed = humantime::parse_duration(expr)
                    .map_err(|err| CronError::InvalidSchedule(err.to_string()))?;
                if parsed.is_zero() {
                    return Err(CronError::InvalidSchedule(
                        "every duration must be greater than zero".to_string(),
                    ));
                }
                Ok(Self::Every(parsed))
            }
        }
    }

    pub fn from_job(job: &CronJob) -> Result<Self, CronError> {
        Self::from_kind_expr(job.schedule_kind, &job.schedule_expr)
    }

    pub fn next_run_after_ms(&self, current_ms: i64) -> Result<i64, CronError> {
        match self {
            Self::Every(interval) => Ok(current_ms.saturating_add(interval.as_millis() as i64)),
            Self::Cron(schedule) => {
                let after = ms_to_utc(current_ms.saturating_add(1));
                let next = schedule.after(&after).next().ok_or_else(|| {
                    CronError::InvalidSchedule("cron expression has no next run".to_string())
                })?;
                Ok(next.timestamp_millis())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::ScheduleSpec;
    use klaw_storage::CronScheduleKind;

    #[test]
    fn parse_every_schedule() {
        let spec = ScheduleSpec::from_kind_expr(CronScheduleKind::Every, "45s").expect("parse");
        let next = spec.next_run_after_ms(1_000).expect("next");
        assert_eq!(next, 46_000);
    }

    #[test]
    fn parse_cron_schedule() {
        let spec =
            ScheduleSpec::from_kind_expr(CronScheduleKind::Cron, "0 */2 * * * *").expect("parse");
        let next = spec.next_run_after_ms(0).expect("next");
        assert_eq!(next, 120_000);
    }
}
