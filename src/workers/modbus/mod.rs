//! Modbus worker modules

pub mod client;
pub mod operations;
pub mod parsing;
pub mod types;
pub mod worker;

pub use client::{ModbusClient, ModbusOp, OperationResult, QueuedOperation};
pub use operations::{parse_register_address, register_type_from_kind, RegisterType};
pub use parsing::{encode_fields_for_partial_write, encode_fields_to_registers, encode_single_field, parse_signal_group_fields, EncodedField, ParsedField};
pub use types::{Backoff, ConnectionState, OperationQueue, TimeoutHandler, TransactionId};
pub use worker::ModbusWorker;
