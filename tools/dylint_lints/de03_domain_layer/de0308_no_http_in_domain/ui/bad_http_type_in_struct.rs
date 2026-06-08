// simulated_dir=/cf-gears/gears/some_gear/domain/service.rs
#![feature(register_tool)]
#![register_tool(dylint)]
#![allow(dead_code)]

pub struct Hello {
    // Should trigger DE0308 - HTTP in domain
    param1: http::StatusCode,
}

fn main() {}
