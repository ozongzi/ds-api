use ds_api::tool;
use serde_json::json;

use super::{MAX_OUTPUT_CHARS, truncate_output};

pub struct FileTool;

#[tool]
impl Tool for FileTool {
    /// 创建文件（若已存在则清空内容）
    /// path: 文件路径
    async fn touch(&self, path: String) -> Value {
        match std::fs::File::create(path) {
            Ok(_) => json!({ "status": "success" }),
            Err(e) => json!({ "error": e.to_string() }),
        }
    }

    /// 删除文件
    /// path: 文件路径
    async fn delete(&self, path: String) -> Value {
        match std::fs::remove_file(path) {
            Ok(_) => json!({ "status": "success" }),
            Err(e) => json!({ "error": e.to_string() }),
        }
    }

    /// 将整个文件内容替换为 content。适合新建文件或大幅重写。
    /// 注意：会覆盖文件全部内容，小改动请用 str_replace。
    /// path: 文件路径
    /// content: 写入的完整内容
    async fn write(&self, path: String, content: String) -> Value {
        // Create parent directories if they don't exist.
        if let Some(parent) = std::path::Path::new(&path).parent() {
            if !parent.as_os_str().is_empty() {
                if let Err(e) = std::fs::create_dir_all(parent) {
                    return json!({ "error": format!("创建目录失败: {e}") });
                }
            }
        }
        match std::fs::write(&path, &content) {
            Ok(_) => {
                let line_count = content.lines().count();
                json!({ "status": "success", "lines_written": line_count })
            }
            Err(e) => json!({ "error": e.to_string() }),
        }
    }

    /// 在文件中精确替换一处文本片段。
    /// old_str 必须在文件中唯一出现；若匹配到多处，返回错误并提示扩大上下文。
    /// 适合局部小改动，比 patch 更直观、不需要计算行号。
    /// path: 文件路径
    /// old_str: 要替换的原始文本（必须与文件内容完全一致，包括空格和换行）
    /// new_str: 替换后的新文本
    async fn str_replace(&self, path: String, old_str: String, new_str: String) -> Value {
        let content = match std::fs::read_to_string(&path) {
            Ok(s) => s,
            Err(e) => return json!({ "error": e.to_string() }),
        };

        let count = content.matches(&old_str as &str).count();
        match count {
            0 => json!({
                "error": "未找到匹配的文本片段，请检查 old_str 是否与文件内容完全一致（包括缩进和换行）"
            }),
            1 => {
                let new_content = content.replacen(&old_str as &str, &new_str, 1);
                match std::fs::write(&path, &new_content) {
                    Ok(_) => json!({ "status": "success" }),
                    Err(e) => json!({ "error": e.to_string() }),
                }
            }
            n => json!({
                "error": format!("找到 {n} 处匹配，old_str 不唯一，请在 old_str 中包含更多上下文使其唯一")
            }),
        }
    }

    /// 获取文件内容。
    /// 若不传 left/right 则返回全文（受 8000 字符限制截断）。
    /// 传入行范围时返回 [left, right) 行（左闭右开，从 0 开始）。
    /// 建议先用 get_file_info 查看行数，再按需分段读取大文件。
    /// path: 文件路径
    /// left: 起始行索引（含），可选，默认 0
    /// right: 结束行索引（不含），可选，默认读到末尾
    async fn get(&self, path: String, left: Option<usize>, right: Option<usize>) -> Value {
        let content = match std::fs::read_to_string(&path) {
            Ok(s) => s,
            Err(e) => return json!({ "error": e.to_string() }),
        };

        let lines: Vec<&str> = content.lines().collect();
        let total = lines.len();
        let l = left.unwrap_or(0);
        let r = right.unwrap_or(total);

        if l > r || r > total {
            return json!({
                "error": format!("索引越界：left={l}, right={r}, 总行数={total}")
            });
        }

        let slice = truncate_output(&lines[l..r].join("\n"), MAX_OUTPUT_CHARS);
        json!({
            "content": slice,
            "lines": { "from": l, "to": r, "total": total },
        })
    }

    /// PATCH文件内容，接受一个左闭右开的行索引范围，将 [left, right) 的内容替换为 new_content。
    /// 若 left == right，则在 left 行前插入内容。
    /// 注意：需要精确的行号，容易越界出错，推荐优先使用 str_replace。
    /// path: 文件路径
    /// left: 左闭行索引
    /// right: 右开行索引
    /// new_content: 替换内容
    async fn patch(&self, path: String, left: usize, right: usize, new_content: String) -> Value {
        if left > right {
            return json!({ "error": "left must be less than or equal to right" });
        }

        let file_content = match std::fs::read_to_string(&path) {
            Ok(s) => s,
            Err(e) => return json!({ "error": e.to_string() }),
        };

        let mut lines: Vec<&str> = file_content.lines().collect();

        if left > lines.len() || right > lines.len() {
            return json!({ "error": format!("索引越界，left: {}, right: {}, 文件行数: {}", left, right, lines.len()) });
        }

        lines.splice(left..right, new_content.lines());
        let updated_content = lines.join("\n");

        match std::fs::write(&path, updated_content) {
            Ok(_) => json!({ "status": "success" }),
            Err(e) => json!({ "error": e.to_string() }),
        }
    }

    /// 获取文件基本信息（大小、行数）。在读取大文件前建议先调用此方法。
    /// path: 文件路径
    async fn get_file_info(&self, path: String) -> Value {
        match std::fs::read_to_string(&path) {
            Ok(content) => {
                let line_count = content.lines().count();
                json!({ "size": content.len(), "lines_number": line_count })
            }
            Err(e) => json!({ "error": e.to_string() }),
        }
    }

    /// 读二进制文件，输出以十六进制显示
    /// path: 文件路径
    /// begin: 起始偏移量
    /// end: 结束偏移量
    async fn read_binary(&self, path: String, begin: usize, end: usize) -> Value {
        match std::fs::read(&path) {
            Ok(content) => {
                let slice = content.get(begin..end).unwrap_or_default();
                let hex = truncate_output(&format!("{:02x?}", slice), MAX_OUTPUT_CHARS);
                json!({ "content": hex })
            }
            Err(e) => json!({ "error": e.to_string() }),
        }
    }

    /// 读二进制文件基本信息
    /// path: 文件路径
    async fn get_binary_info(&self, path: String) -> Value {
        match std::fs::read(&path) {
            Ok(content) => json!({ "size": content.len() }),
            Err(e) => json!({ "error": e.to_string() }),
        }
    }

    /// 创建文件夹
    /// path: 文件夹路径
    async fn create_dir(&self, path: String) -> Value {
        match std::fs::create_dir(&path) {
            Ok(()) => json!({ "success": true }),
            Err(e) => json!({ "error": e.to_string() }),
        }
    }

    /// 递归创建文件夹
    /// path: 文件夹路径
    async fn create_dir_all(&self, path: String) -> Value {
        match std::fs::create_dir_all(&path) {
            Ok(()) => json!({ "success": true }),
            Err(e) => json!({ "error": e.to_string() }),
        }
    }
}
