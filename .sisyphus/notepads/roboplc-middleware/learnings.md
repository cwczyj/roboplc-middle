## Learnings from roboplc-middleware Implementation

### RoboPLC API Usage (CRITICAL)
- **DataPolicy import**: Use `use roboplc::prelude::*;` which re-exports DataPolicy from rtsc. DO NOT use `use roboplc::derive::DataPolicy;` - the derive module doesn't exist.
- **data_delivery variants**: Valid values are `single`, `single_optional`, `optional`, `always`, `latest`. NOT `broadcast` (that's invalid).
- **DataBuffer**: `roboplc::buf::DataBuffer` is a type alias with NO generic params. Use `rtsc::buf::DataBuffer<T>` directly instead.
- **DataBuffer constructor**: Use `DataBuffer::bounded(capacity)` NOT `DataBuffer::new(capacity)`.
- **rtsc crate**: Must add `rtsc = "0.4"` to Cargo.toml for direct DataBuffer access.
- **DataBuffer Debug**: DataBuffer<T> doesn't implement Debug. Implement manual Debug for structs containing DataBuffer fields.

### Fixed Compilation Errors
- Changed `use roboplc::derive::DataPolicy;` to just `use roboplc::prelude::*;`
- Changed `#[data_delivery(broadcast)]` to `#[data_delivery(always)]` (broadcast is invalid)
- Changed `use roboplc::buf::DataBuffer;` to `use rtsc::buf::DataBuffer;`
- Changed `DataBuffer::new(100)` to `DataBuffer::bounded(100)`
- Added `rtsc = "0.4"` to Cargo.toml
- Implemented manual `std::fmt::Debug` for Variables struct since DataBuffer doesn't implement Debug

### Pattern References
- RoboPLC modbus-master example: `/home/lipschitz/.cargo/registry/src/.../roboplc-0.6.4/examples/modbus-master.rs`
- Context7 docs: `/roboplc/roboplc` for Hub, Controller, Worker patterns

### Device Profile Patterns (Task 13)
- Prefer `crate::config::{AddressingMode, ByteOrder, DataType}` for internal modules instead of crate-name imports.
- `AddressMapping::parse` can normalize prefixed Modbus notation (`c`, `d`, `i`, `h`) into typed register space + `u16` offset.
- Keep byte-order handling isolated in a utility (`convert_byte_order`) so converters can stay data-type focused.
- `DataType` is not `Copy`; helper functions that inspect it should accept `&DataType` to avoid move errors.

## 2026-02-26 Task 12 Modbus Worker Scaffolding

- `WorkerOpts` derive must be attached to the worker struct, not the `impl Worker` block.
- A valid Modbus TCP skeleton for RoboPLC can be established with `tcp::connect(endpoint, timeout)?` and a `ModbusMapping` placeholder to anchor `io::modbus::prelude::*` usage.
- Heartbeat loop pattern: `for _ in interval(Duration::from_millis(100)).take_while(|_| context.is_online())` then `context.hub().send(Message::DeviceHeartbeat { ... })`.
