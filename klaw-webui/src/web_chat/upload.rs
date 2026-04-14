use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;
use web_sys::{File, FormData, Request, RequestInit, Response, Url};

use crate::{ArchiveRecord, ArchiveUploadResponse};

pub(super) async fn upload_file_to_archive(
    gateway_origin: &str,
    gateway_token: Option<&str>,
    file: File,
    session_key: &str,
) -> Result<ArchiveRecord, String> {
    let form_data = FormData::new().map_err(|_| "Failed to create FormData")?;

    form_data
        .append_with_blob("file", &file)
        .map_err(|_| "Failed to append file to FormData")?;

    form_data
        .append_with_str("session_key", session_key)
        .map_err(|_| "Failed to append session_key")?;

    let url = format!("{}/archive/upload", gateway_origin);

    let opts = RequestInit::new();
    opts.set_method("POST");
    opts.set_body(&form_data);

    let request =
        Request::new_with_str_and_init(&url, &opts).map_err(|_| "Failed to create request")?;

    if let Some(token) = gateway_token {
        request
            .headers()
            .set("Authorization", &format!("Bearer {}", token))
            .map_err(|_| "Failed to set Authorization header")?;
    }

    let window = web_sys::window().ok_or("No window object")?;
    let resp_value = JsFuture::from(window.fetch_with_request(&request))
        .await
        .map_err(|_| "Fetch failed")?;

    let resp: Response = resp_value
        .dyn_into()
        .map_err(|_| "Failed to cast to Response")?;

    if !resp.ok() {
        return Err(format!("HTTP {}: {}", resp.status(), resp.status_text()));
    }

    let json = JsFuture::from(resp.json().map_err(|_| "Failed to get JSON")?)
        .await
        .map_err(|_| "Failed to parse JSON")?;

    let upload_resp: ArchiveUploadResponse = serde_wasm_bindgen::from_value(json)
        .map_err(|err| format!("Failed to deserialize response: {}", err))?;

    if upload_resp.success {
        upload_resp
            .record
            .ok_or_else(|| "No record in response".to_string())
    } else {
        Err(upload_resp
            .error
            .unwrap_or_else(|| "Upload failed".to_string()))
    }
}

pub(super) async fn preview_archive_file(
    gateway_origin: &str,
    gateway_token: Option<&str>,
    archive_id: &str,
) -> Result<(), String> {
    let url = format!(
        "{}/archive/download/{}",
        gateway_origin,
        urlencoding::encode(archive_id)
    );
    let opts = RequestInit::new();
    opts.set_method("GET");

    let request =
        Request::new_with_str_and_init(&url, &opts).map_err(|_| "Failed to create request")?;

    if let Some(token) = gateway_token {
        request
            .headers()
            .set("Authorization", &format!("Bearer {}", token))
            .map_err(|_| "Failed to set Authorization header")?;
    }

    let window = web_sys::window().ok_or("No window object")?;
    let resp_value = JsFuture::from(window.fetch_with_request(&request))
        .await
        .map_err(|_| "Fetch failed")?;

    let resp: Response = resp_value
        .dyn_into()
        .map_err(|_| "Failed to cast to Response")?;

    if !resp.ok() {
        return Err(format!("HTTP {}: {}", resp.status(), resp.status_text()));
    }

    let blob = JsFuture::from(resp.blob().map_err(|_| "Failed to read blob")?)
        .await
        .map_err(|_| "Failed to resolve blob")?;
    let blob: web_sys::Blob = blob
        .dyn_into()
        .map_err(|_| "Failed to cast response blob")?;

    let object_url =
        Url::create_object_url_with_blob(&blob).map_err(|_| "Failed to create preview URL")?;
    window
        .open_with_url_and_target(&object_url, "_blank")
        .map_err(|_| "Failed to open preview window")?
        .ok_or_else(|| "Browser blocked preview window".to_string())?;
    Ok(())
}
