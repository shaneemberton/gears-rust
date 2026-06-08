// TODO: DE0301 - refactor to remove toolkit_db dependency from domain layer
// This gear currently uses toolkit_db::DbError, DBRunner, DBProvider which violates DDD
#![allow(unknown_lints)]
#![allow(de0301_no_infra_in_domain)]

pub mod error;
pub mod fields;
pub mod local_client;
pub mod repo;
pub mod service;

#[cfg(test)]
mod service_test;
