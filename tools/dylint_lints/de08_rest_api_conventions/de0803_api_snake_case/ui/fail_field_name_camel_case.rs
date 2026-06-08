// simulated_dir=/cf-gears/gears/some_gear/api/rest/dto.rs
#![allow(non_snake_case)]
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub struct BadFieldNameCamelCaseDto {
    // Should trigger DE0803 - DTO fields must not use non-snake_case in serde rename/rename_all
    pub camelCaseField: String,
}

fn main() {}
