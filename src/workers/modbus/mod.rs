//! Modbus worker modules

pub mod types;
pub mod client;
pub mod operations;
pub mod parsing;

// Re-export types from submodules for convenient access
pub use types::{ConnectionState, OperationQueue, TransactionId};

pub use client::{ModbusClient, ModbusOp, OperationResult, QueuedOperation};