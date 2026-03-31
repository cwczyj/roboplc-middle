//! Modbus worker modules
//!
//! ## Module Structure
//!
//! - `mod.rs` - Module exports and re-exports
//! - `client.rs` - ModbusClient - low-level TCP client
//! - `worker.rs` - ModbusWorker/DeviceControlHandler - RoboPLC worker
//! - `handler.rs` - DeviceControl message handling logic
//! - `state.rs` - ModbusWorkerState - connection state management
//! - `operations.rs` - Register operations and address parsing
//! - `parsing.rs` - Signal group encoding/decoding
//! - `types.rs` - Shared types: Backoff, ConnectionState, etc.

pub mod client;
pub mod handler;
pub mod operations;
pub mod parsing;
pub mod state;
pub mod types;
pub mod worker;

pub use client::{ModbusClient, ModbusConnectionPool, ModbusOp, OperationResult};
pub use handler::DeviceControlHandler;
pub use operations::{parse_register_address, register_type_from_kind, RegisterType};
pub use parsing::{
    encode_fields_for_partial_write, encode_fields_to_registers, encode_single_field,
    parse_signal_group_fields, EncodedField, ParsedField,
};
pub use state::ModbusWorkerState;
pub use types::{
    Backoff, ConnectionState, OperationGuard, OperationQueue, TimeoutHandler, TransactionId,
};
pub use worker::ModbusWorker;
