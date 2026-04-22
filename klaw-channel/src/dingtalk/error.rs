use serde_json::Value;
use std::error::Error;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum DingtalkApiError {
    #[error(
        "dingtalk session webhook {context} failed: errcode={errcode} errmsg={errmsg} body={body}"
    )]
    SessionWebhookBusiness {
        context: String,
        errcode: i64,
        errmsg: String,
        body: Value,
    },
}

impl DingtalkApiError {
    #[must_use]
    pub fn is_session_not_found(&self) -> bool {
        matches!(
            self,
            Self::SessionWebhookBusiness {
                errcode: 300001,
                ..
            }
        )
    }
}

pub fn is_session_webhook_session_not_found_error(err: &(dyn Error + 'static)) -> bool {
    err.downcast_ref::<DingtalkApiError>()
        .is_some_and(DingtalkApiError::is_session_not_found)
}
