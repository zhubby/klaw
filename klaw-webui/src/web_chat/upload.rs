use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;
use web_sys::{File, FormData, Request, RequestInit, Response, Url};

use crate::{ArchiveRecord, ArchiveUploadResponse, WebArchiveResource};

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

pub(super) struct ArchivePreviewBlob {
    pub(super) content_type: Option<String>,
    pub(super) bytes: Vec<u8>,
}

pub(super) async fn load_archive_preview(
    gateway_origin: &str,
    gateway_token: Option<&str>,
    archive_id: &str,
) -> Result<ArchivePreviewBlob, String> {
    let (blob, content_type) =
        fetch_archive_blob(gateway_origin, gateway_token, archive_id).await?;
    let bytes = blob_bytes(&blob).await?;
    Ok(ArchivePreviewBlob {
        content_type,
        bytes,
    })
}

pub(super) async fn download_archive_resource(
    gateway_origin: &str,
    gateway_token: Option<&str>,
    resource: &WebArchiveResource,
) -> Result<(), String> {
    let (blob, _) = fetch_archive_blob(gateway_origin, gateway_token, &resource.archive_id).await?;
    let object_url =
        Url::create_object_url_with_blob(&blob).map_err(|_| "Failed to create download URL")?;
    let download_result = trigger_browser_download(
        &object_url,
        resource.filename.as_deref().unwrap_or("download"),
    );
    let _ = Url::revoke_object_url(&object_url);
    download_result
}

async fn fetch_archive_blob(
    gateway_origin: &str,
    gateway_token: Option<&str>,
    archive_id: &str,
) -> Result<(web_sys::Blob, Option<String>), String> {
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

    let content_type = resp
        .headers()
        .get("content-type")
        .map_err(|_| "Failed to read content type")?;

    let blob = JsFuture::from(resp.blob().map_err(|_| "Failed to read blob")?)
        .await
        .map_err(|_| "Failed to resolve blob")?;
    let blob: web_sys::Blob = blob
        .dyn_into()
        .map_err(|_| "Failed to cast response blob")?;

    Ok((blob, content_type))
}

async fn blob_bytes(blob: &web_sys::Blob) -> Result<Vec<u8>, String> {
    let buffer = JsFuture::from(blob.array_buffer())
        .await
        .map_err(|_| "Failed to read blob bytes")?;
    Ok(js_sys::Uint8Array::new(&buffer).to_vec())
}

fn trigger_browser_download(object_url: &str, filename: &str) -> Result<(), String> {
    let window = web_sys::window().ok_or("No window object")?;
    let document = window.document().ok_or("No document object")?;
    let anchor = document
        .create_element("a")
        .map_err(|_| "Failed to create download link")?
        .dyn_into::<web_sys::HtmlAnchorElement>()
        .map_err(|_| "Failed to cast download link")?;

    anchor.set_href(object_url);
    anchor.set_download(filename);

    if let Some(body) = document.body() {
        body.append_child(&anchor)
            .map_err(|_| "Failed to attach download link")?;
        anchor.click();
        body.remove_child(&anchor)
            .map_err(|_| "Failed to remove download link")?;
    } else {
        anchor.click();
    }

    Ok(())
}
