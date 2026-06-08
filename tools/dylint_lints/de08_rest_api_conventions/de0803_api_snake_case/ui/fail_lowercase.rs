// simulated_dir=/cf-gears/gears/some_gear/api/rest/dto.rs
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
// Should trigger DE0803 - DTO fields must not use non-snake_case in serde rename/rename_all
#[serde(rename_all = "lowercase")]
pub struct BadLowercaseDto {
    pub id: String,
}

fn main() {}
