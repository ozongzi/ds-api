mod a2a_tool;
mod command_tool;
mod file_tool;
mod history_tool;
mod outline_tool;
mod present_file;
mod script_tool;
mod search_tool;

pub use a2a_tool::A2aTool;
pub use command_tool::CommandTool;
pub use file_tool::FileTool;
pub use history_tool::HistoryTool;
pub use outline_tool::OutlineTool;
pub use present_file::PresentFileTool;
pub use script_tool::ScriptTool;
pub use search_tool::SearchTool;

pub const MAX_OUTPUT_CHARS: usize = 8_000;

/// 超长输出保留头尾，中间用省略提示替换
pub(super) fn truncate_output(s: &str, max_chars: usize) -> String {
    if s.len() <= max_chars {
        return s.to_string();
    }
    let half = max_chars / 2;
    let head = &s[..half];
    let tail_start = s.len() - half;
    let tail = &s[tail_start..];
    format!(
        "{}\n\n... [输出过长，中间 {} 字节已省略] ...\n\n{}",
        head,
        s.len() - max_chars,
        tail
    )
}
