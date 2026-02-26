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