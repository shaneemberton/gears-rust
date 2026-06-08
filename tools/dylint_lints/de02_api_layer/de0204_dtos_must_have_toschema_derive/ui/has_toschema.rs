// simulated_dir=/cf-gears/gears/some_gear/api/rest/
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[allow(dead_code)]
// Should not trigger DE0204 - DTOs must have ToSchema derive
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct UserDto {
    pub id: String,
}

#[allow(dead_code)]
// Should not trigger DE0204 - DTOs must have ToSchema derive
#[derive(Debug, Serialize, Deserialize, utoipa::ToSchema)]
pub struct ProductDto {
    pub name: String,
}

fn main() {}
