mod a2a_spell;
mod ask_user_spell;
mod command_spell;
mod file_spell;
mod history_spell;
mod manage_mcp_spell;
mod outline_spell;
mod present_file_spell;
mod script_spell;
mod search_spell;

pub use a2a_spell::A2aSpell;
pub use ask_user_spell::AskUserSpell;
pub use command_spell::CommandSpell;
pub use file_spell::FileSpell;
pub use history_spell::HistorySpell;
pub use manage_mcp_spell::ManageMcpSpell;
pub use outline_spell::OutlineSpell;
pub use present_file_spell::PresentFileSpell;
pub use script_spell::ScriptSpell;
pub use search_spell::SearchSpell;

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
