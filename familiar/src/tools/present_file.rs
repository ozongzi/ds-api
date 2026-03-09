use ds_api::tool;
use serde_json::json;

pub struct PresentFileTool;

#[tool]
impl Tool for PresentFileTool {
    /// 将服务器上的文件作为附件发送到 Discord 频道。
    /// 适合展示代码、日志、生成的图片等任何文件。
    /// path: 要发送的文件的绝对路径或相对路径
    async fn present_file(&self, path: String) -> Value {
        match tokio::fs::read(&path).await {
            Ok(bytes) => {
                let filename = std::path::Path::new(&path)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("file")
                    .to_string();
                // 把文件内容 base64 编码后返回，让 bot 层识别并上传
                let encoded = BASE64.encode(&bytes);
                json!({
                    "upload": true,
                    "filename": filename,
                    "data_base64": encoded,
                    "size": bytes.len(),
                })
            }
            Err(e) => json!({ "error": e.to_string() }),
        }
    }
}

use base64::{Engine, engine::general_purpose::STANDARD as BASE64};
