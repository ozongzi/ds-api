use std::{
    ops::Add,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use crate::raw::ChatCompletionResponse;

pub trait Response {
    fn content(&self) -> &str;
    fn created(&self) -> SystemTime;
}

impl Response for ChatCompletionResponse {
    fn content(&self) -> &str {
        &self.choices[0].message.content.as_ref().unwrap()
    }

    fn created(&self) -> SystemTime {
        UNIX_EPOCH.add(Duration::from_secs(self.created))
    }
}
