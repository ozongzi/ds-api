use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Model {
    DeepseekChat,
    DeepseekReasoner,
}

impl Default for Model {
    fn default() -> Self {
        Model::DeepseekChat
    }
}
