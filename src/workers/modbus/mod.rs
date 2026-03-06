//! Modbus worker modules

pub mod client;
pub mod operations;
pub mod parsing;
pub mod types;
pub mod worker;

pub use types::{Backoff, ConnectionState, OperationQueue, TimeoutHandler, TransactionId};
pub use client::{ModbusClient, ModbusOp, OperationResult, QueuedOperation};
pub use operations::{parse_register_address, register_type_from_kind, RegisterType};
pub use parsing::{encode_fields_to_registers, parse_signal_group_fields, ParsedField};
pub use worker::ModbusWorker;
