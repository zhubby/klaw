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
