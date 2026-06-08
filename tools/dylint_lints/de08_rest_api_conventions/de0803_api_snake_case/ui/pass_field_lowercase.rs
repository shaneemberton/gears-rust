// simulated_dir=/cf-gears/gears/some_gear/api/rest/dto.rs
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub struct GoodFieldLowercaseDto {
    // Should not trigger DE0803 - DTO fields must not use non-snake_case in serde rename/rename_all
    #[serde(rename = "lowercase")]
    pub id: String,
}

fn main() {}
