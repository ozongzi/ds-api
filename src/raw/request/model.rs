use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
#[derive(Default)]
pub enum Model {
    #[default]
    DeepseekChat,
    DeepseekReasoner,
}

