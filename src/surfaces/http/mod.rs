pub mod mcp;
pub mod rest;
pub mod server;
pub mod web;

pub use server::{build_router, serve};
