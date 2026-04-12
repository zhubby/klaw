use anyhow::Result;
use reqwest::multipart;
use std::env;

#[tokio::main]
async fn main() -> Result<()> {
    let base_url = env::var("BASE_URL").unwrap_or_else(|_| "http://127.0.0.1:18080".to_string());
    let token = env::var("GATEWAY_TOKEN").unwrap_or_else(|_| "test-token".to_string());

    let client = reqwest::Client::new();

    // Create a test file content
    let file_content = b"Hello, this is a test file for archive upload!";
    let file_part = multipart::Part::bytes(file_content.to_vec())
        .file_name("test.txt")
        .mime_str("text/plain")?;

    let form = multipart::Form::new()
        .part("file", file_part)
        .text("session_key", "terminal:test")
        .text("channel", "terminal")
        .text("chat_id", "test-chat");

    println!("Uploading file to {}/archive/upload", base_url);

    let response = client
        .post(format!("{}/archive/upload", base_url))
        .header("Authorization", format!("Bearer {}", token))
        .multipart(form)
        .send()
        .await?;

    let status = response.status();
    let body = response.text().await?;

    println!("Status: {}", status);
    println!("Response: {}", body);

    if status.is_success() {
        let json: serde_json::Value = serde_json::from_str(&body)?;
        if let Some(record) = json.get("record") {
            if let Some(id) = record.get("id").and_then(|v| v.as_str()) {
                println!("\n✓ File uploaded successfully!");
                println!("Archive ID: {}", id);
                println!("\nYou can download it with:");
                println!(
                    "curl -H 'Authorization: Bearer {}' {}/archive/download/{} -o downloaded.txt",
                    token, base_url, id
                );
            }
        }
    }

    Ok(())
}
