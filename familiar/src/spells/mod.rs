mod a2a_spell;
mod command_spell;
mod file_spell;
mod history_spell;
mod outline_spell;
mod present_file_spell;
mod script_spell;
mod search_spell;

pub use a2a_spell::A2aSpell;
pub use command_tool::CommandSpell;
pub use file_tool::FileSpell;
pub use history_tool::HistorySpell;
pub use outline_tool::OutlineSpell;
pub use present_file::PresentFileSpell;
pub use script_tool::ScriptSpell;
pub use search_tool::SearchSpell;

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
