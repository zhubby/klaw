use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{Tool, ToolCategory, ToolContext, ToolError, ToolOutput};

const GEO_TIMEOUT_SECONDS: u64 = 60;

pub struct GeoTool;

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct GeoRequest {}

#[derive(Debug, Serialize)]
struct GeoResponse {
    latitude: f64,
    longitude: f64,
    horizontal_accuracy_meters: Option<f64>,
    vertical_accuracy_meters: Option<f64>,
    altitude_meters: Option<f64>,
    timestamp_unix_seconds: f64,
    source: &'static str,
    accuracy_authorization: Option<&'static str>,
}

impl GeoTool {
    #[must_use]
    pub fn new() -> Self {
        Self
    }

    fn parse_request(args: Value) -> Result<GeoRequest, ToolError> {
        serde_json::from_value(args)
            .map_err(|err| ToolError::InvalidArgs(format!("invalid request: {err}")))
    }

    fn format_user_message(location: &GeoResponse) -> String {
        let mut message = format!(
            "Current coordinates: {:.6}, {:.6}",
            location.latitude, location.longitude
        );
        if let Some(horizontal_accuracy) = location.horizontal_accuracy_meters {
            message.push_str(&format!(
                " (horizontal accuracy: {:.0}m)",
                horizontal_accuracy
            ));
        }
        message
    }
}

#[async_trait]
impl Tool for GeoTool {
    fn name(&self) -> &str {
        "geo"
    }

    fn description(&self) -> &str {
        "Get the current coordinates from available system location services."
    }

    fn parameters(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {},
            "additionalProperties": false,
            "description": "Returns the current latitude and longitude from the host location services."
        })
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Hardware
    }

    async fn execute(
        &self,
        args: serde_json::Value,
        _ctx: &ToolContext,
    ) -> Result<ToolOutput, ToolError> {
        let _request = Self::parse_request(args)?;

        #[cfg(target_os = "macos")]
        {
            let location = fetch_current_location_macos().await?;
            let content_for_model = serde_json::to_string_pretty(&location).map_err(|err| {
                ToolError::ExecutionFailed(format!("geo serialization failed: {err}"))
            })?;
            return Ok(ToolOutput {
                content_for_user: Some(Self::format_user_message(&location)),
                media: Vec::new(),
            signals: Vec::new(),
                content_for_model,
            });
        }

        #[cfg(not(target_os = "macos"))]
        {
            Err(ToolError::ExecutionFailed(
                "geo is currently only supported on macOS".to_string(),
            ))
        }
    }
}

#[cfg(target_os = "macos")]
async fn fetch_current_location_macos() -> Result<GeoResponse, ToolError> {
    macos::fetch_current_location_with_authorization_strategy().await
}

#[cfg(target_os = "macos")]
mod macos {
    use std::cell::RefCell;
    use std::sync::Mutex;
    use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
    use std::time::Duration;

    use objc2::rc::Retained;
    use objc2::runtime::ProtocolObject;
    use objc2::{ClassType, DeclaredClass, declare_class, msg_send_id, mutability};
    use objc2_core_location::{
        CLAccuracyAuthorization, CLAuthorizationStatus, CLError, CLLocation, CLLocationManager,
        CLLocationManagerDelegate, kCLLocationAccuracyBest,
    };
    use objc2_foundation::{NSArray, NSError, NSObject, NSObjectProtocol, run_on_main};

    use super::{GEO_TIMEOUT_SECONDS, GeoResponse};
    use crate::ToolError;

    const LOCATION_UNKNOWN_RETRY_LIMIT: usize = 2;

    thread_local! {
        static ACTIVE_REQUEST: RefCell<Option<ActiveGeoRequest>> = const { RefCell::new(None) };
    }

    struct ActiveGeoRequest {
        manager: Retained<CLLocationManager>,
        delegate: Retained<LocationDelegate>,
    }

    #[derive(Debug)]
    struct LocationDelegateState {
        sender: Mutex<Option<Sender<Result<GeoResponse, String>>>>,
        location_unknown_retries_remaining: Mutex<usize>,
    }

    declare_class!(
        struct LocationDelegate;

        // SAFETY:
        // - `NSObject` has no special subclassing requirements for this delegate.
        // - The ivars are guarded by `Mutex`, so interior mutability is appropriate.
        // - The type does not implement `Drop`.
        unsafe impl ClassType for LocationDelegate {
            type Super = NSObject;
            type Mutability = mutability::InteriorMutable;
            const NAME: &'static str = "KlawGeoLocationDelegate";
        }

        impl DeclaredClass for LocationDelegate {
            type Ivars = LocationDelegateState;
        }

        unsafe impl NSObjectProtocol for LocationDelegate {}

        unsafe impl CLLocationManagerDelegate for LocationDelegate {
            #[method(locationManager:didUpdateLocations:)]
            fn location_manager_did_update_locations(
                &self,
                manager: &CLLocationManager,
                locations: &NSArray<CLLocation>,
            ) {
                let result = unsafe { locations.lastObject() }
                    .ok_or_else(|| "macOS location services returned no coordinates".to_string())
                    .and_then(|location| GeoResponse::from_location(manager, &location));
                self.complete(manager, result);
            }

            #[method(locationManager:didFailWithError:)]
            fn location_manager_did_fail_with_error(
                &self,
                manager: &CLLocationManager,
                error: &NSError,
            ) {
                if self.retry_location_unknown(manager, error) {
                    return;
                }

                self.complete(manager, Err(format_location_error(error)));
            }

            #[method(locationManagerDidChangeAuthorization:)]
            fn location_manager_did_change_authorization(&self, manager: &CLLocationManager) {
                match authorization_state(unsafe { manager.authorizationStatus() }) {
                    AuthorizationState::Authorized => unsafe {
                        manager.startUpdatingLocation();
                    },
                    AuthorizationState::Denied => {
                        self.complete(manager, Err(
                            "macOS location permission was denied or restricted".to_string(),
                        ));
                    }
                    AuthorizationState::NotDetermined => {}
                }
            }
        }
    );

    impl LocationDelegate {
        fn new(sender: Sender<Result<GeoResponse, String>>) -> Retained<Self> {
            let this = Self::alloc().set_ivars(LocationDelegateState {
                sender: Mutex::new(Some(sender)),
                location_unknown_retries_remaining: Mutex::new(LOCATION_UNKNOWN_RETRY_LIMIT),
            });
            unsafe { msg_send_id![super(this), init] }
        }

        fn retry_location_unknown(&self, manager: &CLLocationManager, error: &NSError) -> bool {
            if cl_error_code(error) != Some(CLError::kCLErrorLocationUnknown) {
                return false;
            }

            let mut retries_remaining = self
                .ivars()
                .location_unknown_retries_remaining
                .lock()
                .unwrap_or_else(|err| err.into_inner());
            if *retries_remaining == 0 {
                return false;
            }

            *retries_remaining -= 1;
            unsafe {
                manager.startUpdatingLocation();
            }
            true
        }

        fn complete(&self, manager: &CLLocationManager, result: Result<GeoResponse, String>) {
            let mut sender = self
                .ivars()
                .sender
                .lock()
                .unwrap_or_else(|err| err.into_inner());
            if let Some(sender) = sender.take() {
                let _ = sender.send(result);
            }
            unsafe {
                manager.stopUpdatingLocation();
                manager.setDelegate(None);
            }
            clear_active_request();
        }
    }

    impl GeoResponse {
        fn from_location(
            manager: &CLLocationManager,
            location: &CLLocation,
        ) -> Result<Self, String> {
            let coordinate = unsafe { location.coordinate() };
            if !unsafe { objc2_core_location::CLLocationCoordinate2DIsValid(coordinate) }.as_bool()
            {
                return Err("macOS location services returned invalid coordinates".to_string());
            }

            let horizontal_accuracy =
                non_negative_accuracy(unsafe { location.horizontalAccuracy() });
            let vertical_accuracy = non_negative_accuracy(unsafe { location.verticalAccuracy() });
            let altitude = finite_value(unsafe { location.altitude() });
            let timestamp_unix_seconds = unsafe { location.timestamp().timeIntervalSince1970() };

            Ok(Self {
                latitude: coordinate.latitude,
                longitude: coordinate.longitude,
                horizontal_accuracy_meters: horizontal_accuracy,
                vertical_accuracy_meters: vertical_accuracy,
                altitude_meters: altitude,
                timestamp_unix_seconds,
                source: "core_location",
                accuracy_authorization: Some(accuracy_authorization_label(unsafe {
                    manager.accuracyAuthorization()
                })),
            })
        }
    }

    pub(super) async fn fetch_current_location_with_authorization_strategy()
    -> Result<GeoResponse, ToolError> {
        let status = current_authorization_status_on_main_thread().await?;
        match authorization_state(status) {
            AuthorizationState::Authorized | AuthorizationState::NotDetermined => {}
            AuthorizationState::Denied => {}
        }

        if authorization_state(status) == AuthorizationState::Denied {
            return Err(ToolError::ExecutionFailed(format!(
                "location permission is unavailable (services_enabled: {}, authorization_status: {} [{}])",
                unsafe { CLLocationManager::locationServicesEnabled_class() },
                authorization_status_label(status),
                status.0
            )));
        }

        let (sender, receiver) = mpsc::channel();
        start_location_request_on_main_thread(sender).await?;
        wait_for_location_result(receiver).await
    }

    async fn current_authorization_status_on_main_thread()
    -> Result<CLAuthorizationStatus, ToolError> {
        tokio::task::spawn_blocking(|| run_on_main(|_| current_authorization_status()))
            .await
            .map_err(|err| ToolError::ExecutionFailed(format!("geo task failed: {err}")))
    }

    async fn start_location_request_on_main_thread(
        sender: Sender<Result<GeoResponse, String>>,
    ) -> Result<(), ToolError> {
        tokio::task::spawn_blocking(|| {
            run_on_main(|_| {
                let manager = unsafe { CLLocationManager::new() };
                let delegate = LocationDelegate::new(sender);

                ACTIVE_REQUEST.with(|slot| {
                    let mut slot = slot.borrow_mut();
                    *slot = Some(ActiveGeoRequest { manager, delegate });

                    let active = slot
                        .as_mut()
                        .expect("active geo request must be present after insertion");
                    let delegate_ref = ProtocolObject::from_ref(&*active.delegate);
                    unsafe {
                        active.manager.setDelegate(Some(delegate_ref));
                        active.manager.setDesiredAccuracy(kCLLocationAccuracyBest);
                    }

                    let start_result = start_location_flow(&active.manager, &active.delegate);
                    if start_result.is_err() {
                        unsafe {
                            active.manager.setDelegate(None);
                        }
                        slot.take();
                    }
                    start_result
                })
            })
        })
        .await
        .map_err(|err| ToolError::ExecutionFailed(format!("geo task failed: {err}")))?
    }

    async fn wait_for_location_result(
        receiver: Receiver<Result<GeoResponse, String>>,
    ) -> Result<GeoResponse, ToolError> {
        let result = tokio::task::spawn_blocking(move || {
            receiver.recv_timeout(Duration::from_secs(GEO_TIMEOUT_SECONDS))
        })
        .await
        .map_err(|err| ToolError::ExecutionFailed(format!("geo task failed: {err}")))?;

        match result {
            Ok(Ok(location)) => Ok(location),
            Ok(Err(message)) => Err(ToolError::ExecutionFailed(message)),
            Err(RecvTimeoutError::Timeout) => {
                clear_active_request_on_main_thread().await;
                let status = current_authorization_status_on_main_thread().await?;
                Err(ToolError::ExecutionFailed(format!(
                    "timed out after {}s waiting for location services (services_enabled: {}, authorization_status: {} [{}])",
                    GEO_TIMEOUT_SECONDS,
                    unsafe { CLLocationManager::locationServicesEnabled_class() },
                    authorization_status_label(status),
                    status.0
                )))
            }
            Err(RecvTimeoutError::Disconnected) => Err(ToolError::ExecutionFailed(
                "macOS location delegate disconnected unexpectedly".to_string(),
            )),
        }
    }

    fn current_authorization_status() -> CLAuthorizationStatus {
        unsafe { CLLocationManager::new().authorizationStatus() }
    }

    fn start_location_flow(
        manager: &CLLocationManager,
        delegate: &LocationDelegate,
    ) -> Result<(), ToolError> {
        if !unsafe { CLLocationManager::locationServicesEnabled_class() } {
            delegate.complete(
                manager,
                Err("macOS location services are disabled for this device".to_string()),
            );
            return Ok(());
        }

        match authorization_state(unsafe { manager.authorizationStatus() }) {
            AuthorizationState::Authorized => unsafe {
                manager.startUpdatingLocation();
            },
            AuthorizationState::NotDetermined => unsafe {
                manager.requestWhenInUseAuthorization();
            },
            AuthorizationState::Denied => {
                return Err(ToolError::ExecutionFailed(format!(
                    "location permission is unavailable (services_enabled: {}, authorization_status: {} [{}])",
                    unsafe { CLLocationManager::locationServicesEnabled_class() },
                    authorization_status_label(unsafe { manager.authorizationStatus() }),
                    unsafe { manager.authorizationStatus() }.0
                )));
            }
        }

        Ok(())
    }

    async fn clear_active_request_on_main_thread() {
        let _ = tokio::task::spawn_blocking(|| run_on_main(|_| clear_active_request())).await;
    }

    fn clear_active_request() {
        ACTIVE_REQUEST.with(|slot| {
            slot.borrow_mut().take();
        });
    }

    fn format_location_error(error: &NSError) -> String {
        match cl_error_code(error) {
            Some(code) if code == CLError::kCLErrorLocationUnknown => format!(
                "location temporarily unavailable (kCLErrorLocationUnknown); try again shortly (domain: {}, code: {})",
                error.domain(),
                error.code()
            ),
            Some(code) if code == CLError::kCLErrorDenied => format!(
                "location access denied by system location services (domain: {}, code: {})",
                error.domain(),
                error.code()
            ),
            Some(code) if code == CLError::kCLErrorNetwork => format!(
                "location lookup failed due to a network-related issue (domain: {}, code: {})",
                error.domain(),
                error.code()
            ),
            _ => format!(
                "{} (domain: {}, code: {})",
                error.localizedDescription(),
                error.domain(),
                error.code()
            ),
        }
    }

    fn cl_error_code(error: &NSError) -> Option<CLError> {
        let code = error.code();
        Some(CLError(code))
    }

    fn finite_value(value: f64) -> Option<f64> {
        value.is_finite().then_some(value)
    }

    fn non_negative_accuracy(value: f64) -> Option<f64> {
        (value.is_finite() && value >= 0.0).then_some(value)
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum AuthorizationState {
        NotDetermined,
        Authorized,
        Denied,
    }

    fn authorization_state(status: CLAuthorizationStatus) -> AuthorizationState {
        if status == CLAuthorizationStatus::kCLAuthorizationStatusNotDetermined {
            AuthorizationState::NotDetermined
        } else if status == CLAuthorizationStatus::kCLAuthorizationStatusAuthorizedAlways
            || status == CLAuthorizationStatus::kCLAuthorizationStatusAuthorizedWhenInUse
        {
            AuthorizationState::Authorized
        } else {
            AuthorizationState::Denied
        }
    }

    fn authorization_status_label(status: CLAuthorizationStatus) -> &'static str {
        if status == CLAuthorizationStatus::kCLAuthorizationStatusNotDetermined {
            "not_determined"
        } else if status == CLAuthorizationStatus::kCLAuthorizationStatusRestricted {
            "restricted"
        } else if status == CLAuthorizationStatus::kCLAuthorizationStatusDenied {
            "denied"
        } else if status == CLAuthorizationStatus::kCLAuthorizationStatusAuthorizedAlways {
            "authorized_always"
        } else if status == CLAuthorizationStatus::kCLAuthorizationStatusAuthorizedWhenInUse {
            "authorized_when_in_use"
        } else {
            "unknown"
        }
    }

    fn accuracy_authorization_label(value: CLAccuracyAuthorization) -> &'static str {
        if value == CLAccuracyAuthorization::ReducedAccuracy {
            "reduced"
        } else {
            "full"
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn geo_rejects_unknown_fields() {
        let err = GeoTool::parse_request(json!({ "unexpected": true }))
            .expect_err("unknown fields should fail");
        assert_eq!(err.code(), "invalid_args");
    }

    #[test]
    fn geo_formats_user_message() {
        let response = GeoResponse {
            latitude: 12.34,
            longitude: 56.78,
            horizontal_accuracy_meters: Some(42.0),
            vertical_accuracy_meters: None,
            altitude_meters: None,
            timestamp_unix_seconds: 1.0,
            source: "core_location",
            accuracy_authorization: Some("full"),
        };

        assert_eq!(
            GeoTool::format_user_message(&response),
            "Current coordinates: 12.340000, 56.780000 (horizontal accuracy: 42m)"
        );
    }
}
