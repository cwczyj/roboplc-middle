# Task 1: Remove MoveTo from messages.rs

## Completed
- Removed `Operation::MoveTo` variant from enum (was line 152)
- Removed `test_operation_move_to_serialization` test (was lines 201-208)
- Removed `test_device_control_clone_with_move_to` test (was lines 256-271)

## Verification
- `grep -r "MoveTo" src/messages.rs` returns no results
- `Operation` enum now contains: `ReadSignalGroup`, `WriteSignalGroup`, `GetStatus`
- File reduced from 272 to 246 lines

## Note
Build errors in rpc_worker.rs and modbus/worker.rs are expected - those files still reference MoveTo and will be cleaned up in Task 4 as per the plan.


# Task 2: WriteValue enum

## Completed
- Added `WriteValue` enum at lines 30-34 with `Coil(bool)` and `Holding(u16)` variants
- Enum derives `Debug`, `Clone`, `PartialEq`

## Verification
- Build passes with expected errors from Task 1 (MoveTo removal)

# Task 3: Unified write_registers method

## Completed
- Added `write_registers` method to `ModbusClient` impl (lines 374-466)
- Method signature: `fn write_registers(&self, client: &Client, address: u16, values: &[WriteValue]) -> OperationResult`
- FC auto-selection:
  - Single value: FC05 (Write Single Coil) or FC06 (Write Single Holding)
  - Multiple values: FC15 (Write Multiple Coils) or FC16 (Write Multiple Holdings)
- Validation:
  - Returns error for empty values slice
  - Returns error for mixed Coil/Holding types (must be homogeneous)
- Reuses existing `write_single`, `write_multiple`, `write_single_coil`, `write_multiple_coils` methods

## Implementation Notes
- Used `matches!` macro for homogeneous type checking
- Used `filter_map` for type-safe value extraction
- Existing write methods kept for now (Task 4 will update callers)

## Verification
- Build expected to have MoveTo errors from Task 1 (fixed in Task 4)
