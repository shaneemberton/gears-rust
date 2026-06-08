// simulated_dir=/cf-gears/gears/some_gear/contract/
#[derive(Debug, Clone)]
#[allow(dead_code)]
// Should not trigger DE0103 - HTTP types in contract
pub enum OrderStatus {
    Pending,
    Confirmed,
    Shipped,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
// Should not trigger DE0103 - HTTP types in contract
pub struct OrderResult {
    pub status: OrderStatus,
}

fn main() {}
