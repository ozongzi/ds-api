use ds_api::tool;
use serde_json::json;

pub struct FileTool;

#[tool]
impl Tool for FileTool {
    /// 创建文件
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

    /// PATCH文件内容，接受一个左闭右开的行索引范围，将 [left, right) 的内容替换为 new_content。若 left == right，则在 left 前插入内容
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

    /// 获取文件内容（按行范围）
    /// path: 文件路径
    /// left: 左闭行索引
    /// right: 右开行索引
    async fn get(&self, path: String, left: usize, right: usize) -> Value {
        match std::fs::read_to_string(&path) {
            Ok(content) => {
                let lines: Vec<&str> = content.lines().collect();

                if left > lines.len() || right > lines.len() || left >= right {
                    return json!({ "error": "out of bounds" });
                }

                let content = lines[left..right].join("\n");
                json!({ "content": content })
            }
            Err(e) => json!({ "error": e.to_string() }),
        }
    }

    /// 获取文件基本信息（大小、行数）
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
                json!({ "content": format!("{:02x?}", slice) })
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
