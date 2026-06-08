// simulated_dir=/cf-gears/gears/some_gear/api/rest/dto.rs
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct GoodDto {
    pub id: String,
}

#[derive(Serialize, Deserialize)]
pub struct DefaultDto {
    pub id: String,
}

fn main() {}
