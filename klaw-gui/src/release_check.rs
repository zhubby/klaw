use semver::Version;
use serde::Deserialize;
use std::time::Duration;

pub const LATEST_RELEASE_API_URL: &str = "https://api.github.com/repos/zhubby/klaw/releases/latest";
const RELEASE_NAME_PREFIX: &str = "release-v";
const RELEASE_CHECK_TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseUpdateInfo {
    pub current_version: String,
    pub latest_version: String,
    pub release_name: String,
    pub release_url: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReleaseCheckOutcome {
    UpToDate,
    UpdateAvailable(ReleaseUpdateInfo),
}

#[derive(Debug, Deserialize)]
struct GithubRelease {
    name: String,
    html_url: String,
}

pub fn check_for_release_update() -> Result<ReleaseCheckOutcome, String> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|err| format!("failed to build release-check runtime: {err}"))?;
    runtime.block_on(async_check_for_release_update())
}

async fn async_check_for_release_update() -> Result<ReleaseCheckOutcome, String> {
    let client = reqwest::Client::builder()
        .timeout(RELEASE_CHECK_TIMEOUT)
        .build()
        .map_err(|err| format!("failed to build release-check client: {err}"))?;
    let release = client
        .get(LATEST_RELEASE_API_URL)
        .header(reqwest::header::ACCEPT, "application/vnd.github+json")
        .header(
            reqwest::header::USER_AGENT,
            format!("klaw-gui/{}", env!("CARGO_PKG_VERSION")),
        )
        .send()
        .await
        .map_err(|err| format!("failed to fetch latest release: {err}"))?
        .error_for_status()
        .map_err(|err| format!("latest release request failed: {err}"))?
        .json::<GithubRelease>()
        .await
        .map_err(|err| format!("failed to decode latest release payload: {err}"))?;

    let current_version = parse_semver(env!("CARGO_PKG_VERSION"))?;
    let latest_version = parse_release_name_version(&release.name)?;

    if latest_version > current_version {
        return Ok(ReleaseCheckOutcome::UpdateAvailable(ReleaseUpdateInfo {
            current_version: current_version.to_string(),
            latest_version: latest_version.to_string(),
            release_name: release.name,
            release_url: release.html_url,
        }));
    }

    Ok(ReleaseCheckOutcome::UpToDate)
}

fn parse_release_name_version(name: &str) -> Result<Version, String> {
    let raw_version = name
        .strip_prefix(RELEASE_NAME_PREFIX)
        .ok_or_else(|| format!("release name must start with `{RELEASE_NAME_PREFIX}`: {name}"))?;
    parse_semver(raw_version)
}

fn parse_semver(raw_version: &str) -> Result<Version, String> {
    Version::parse(raw_version)
        .map_err(|err| format!("invalid semantic version `{raw_version}`: {err}"))
}

#[cfg(test)]
mod tests {
    use super::{ReleaseCheckOutcome, ReleaseUpdateInfo, parse_release_name_version};

    #[test]
    fn parses_release_name_versions() {
        let version = parse_release_name_version("release-v0.8.3").expect("valid release name");
        assert_eq!(version.major, 0);
        assert_eq!(version.minor, 8);
        assert_eq!(version.patch, 3);
    }

    #[test]
    fn rejects_invalid_release_name_prefix() {
        let error = parse_release_name_version("v0.8.3").expect_err("missing release prefix");
        assert!(error.contains("release-v"));
    }

    #[test]
    fn update_outcome_carries_release_metadata() {
        let outcome = ReleaseCheckOutcome::UpdateAvailable(ReleaseUpdateInfo {
            current_version: "0.8.3".to_string(),
            latest_version: "0.8.4".to_string(),
            release_name: "release-v0.8.4".to_string(),
            release_url: "https://github.com/zhubby/klaw/releases/tag/v0.8.4".to_string(),
        });

        match outcome {
            ReleaseCheckOutcome::UpdateAvailable(info) => {
                assert_eq!(info.current_version, "0.8.3");
                assert_eq!(info.latest_version, "0.8.4");
            }
            ReleaseCheckOutcome::UpToDate => panic!("expected update"),
        }
    }
}
