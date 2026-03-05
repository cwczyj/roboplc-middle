# Draft: Modbus Worker Refactoring

## Requirements Summary

Based on analysis of `src/workers/modbus_worker.rs` and user requirements:

1. **Register Content Parsing** - Currently missing
   - `SignalGroup.fields` define data types and offsets but aren't used
   - Raw `u16` values returned without parsing into `U32`, `F32`, etc.
   - Need to implement multi-register parsing (e.g., 2 registers → F32)

2. **All Register Types** - Currently only Holding registers
   - Support: Coils (c), Discrete Inputs (d), Input Registers (i), Holding Registers (h)
   - Use `ModbusRegisterKind` enum variants: `Coil`, `Discrete`, `Input`, `Holding`
   - Each has different read/write permissions

3. **Remove MoveTo Operation** - No longer needed
   - Signal groups replace the need for MoveTo
   - Remove from `Operation` enum handling

4. **Continue Current Approach** - Not using roboplc binrw
   - User prefers current manual approach over `#[binrw]` derive
   - Implement parsing logic manually using `DataType` enum
   - Use `profiles::device_profile::DataTypeConverter` helper

5. **Code Restructuring** - Improve organization
   - Split into logical modules/functions
   - Remove unused code (OperationQueue, profiles if unused)
   - Better separation of concerns

## Technical Findings

### RoboPLC Modbus API (v0.6.4)

**ModbusRegisterKind enum**:
```rust
pub enum ModbusRegisterKind {
    Coil,      // Boolean, read/write (0x addresses)
    Discrete,  // Boolean, read-only (1x addresses)
    Input,     // 16-bit, read-only (3x addresses)
    Holding,   // 16-bit, read/write (4x addresses)
}
```

**String notation**:
- `"c0"` → Coil at address 0
- `"d5"` → Discrete Input at address 5
- `"i10"` → Input Register at address 10
- `"h100"` → Holding Register at address 100

**ModbusMapping API**:
```rust
// Create mapping for any register type
let mapping = ModbusMapping::create(&client, unit_id, "c10", 5)?;

// Read/write operations
let values: Vec<u16> = mapping.read()?;
mapping.write(values)?;
```

### Current Implementation Issues

**Line 295-341**: `read_holding()` only handles `ModbusRegisterKind::Holding`
- Hardcoded to holding registers
- No support for coils, discrete inputs, input registers

**Line 309-323**: Manual register-by-register reading
- Inefficient: creates new ModbusMapping for each register
- Should read all registers in one operation

**Line 584-620**: `operation_to_modbus_op()` only maps to `ReadHolding`/`WriteMultiple`
- Doesn't parse register type from address prefix
- Always uses holding registers

**Line 622-644**: `parse_address()` extracts number but ignores prefix
- Prefix parsed but not used to determine register type

### Available Helper Code

**src/profiles/device_profile.rs**:
- `RegisterType` enum matches ModbusRegisterKind
- `DataTypeConverter` trait for parsing bytes to values
- `DefaultDataTypeConverter` implementation handles U16/I16/U32/I32/F32/Bool
- `convert_byte_order()` handles BigEndian/LittleEndian/etc

## Open Questions

### Question 1: Register Parsing Strategy

For `SignalGroup` with multiple fields, should we:

**Option A**: Read all registers at once, then parse fields
- More efficient (single Modbus read)
- Parse in memory based on field offsets and data types
- Example: Read 10 registers, extract fields at offsets 0, 2, 4

**Option B**: Read each field separately
- Simpler implementation
- More network overhead
- Better for sparse field layouts

**Option C**: Hybrid approach
- Group consecutive fields into single reads
- Read non-consecutive fields separately

### Question 2: Data Type Conversion

How should we handle byte order and data conversion?

**Option A**: Use existing `DataTypeConverter` from profiles
- Already implemented in `device_profile.rs`
- Handles all data types and byte orders
- Well-tested code

**Option B**: Create new converter in modbus_worker
- Keep modbus_worker self-contained
- Avoid dependency on profiles module
- Duplicate conversion logic

**Option C**: Move converter to shared module
- Create `src/data_conversion.rs`
- Both modbus_worker and profiles use it
- Single source of truth

### Question 3: Register Type Support

Should `SignalGroup` support mixed register types?

**Current**: `register_address` is single address (e.g., "h100")
- All fields must be in same register type and consecutive

**Option A**: Keep current design
- One SignalGroup = one register type + consecutive addresses
- Simpler, matches typical PLC usage
- Create multiple SignalGroups for different register types

**Option B**: Allow mixed register types
- Each field specifies own register type
- More flexible but complex
- Requires multiple Modbus reads per group

### Question 4: Write Operations

For writing SignalGroups with different data types:

**Option A**: Accept JSON with field names → convert → write
- API: `{"temperature": 25.5, "status": 100}`
- Worker converts to register values based on field config
- Handles byte order and multi-register values

**Option B**: Accept raw register values
- API: `{"values": [100, 200, 300]}`
- Caller responsible for conversion
- Simpler but pushes complexity to caller

**Option C**: Both approaches
- Provide field-based API for convenience
- Provide raw values API for advanced use
- More code but more flexible

### Question 5: Code Organization

How to restructure the 1100-line modbus_worker.rs?

**Option A**: Single file, better organized
- Split into clear sections with comments
- Extract helper functions
- Keep all in one file

**Option B**: Module split
- `src/workers/modbus/` directory
- `mod.rs` - main ModbusWorker
- `client.rs` - ModbusClient
- `operations.rs` - ModbusOp, operation handling
- `parsing.rs` - register parsing logic

**Option C**: Keep single file, extract utilities
- Main worker logic in modbus_worker.rs
- Move helpers to `src/modbus_helpers.rs`
- DataTypeConverter to shared module

### Question 6: Profiles Module

The `profiles` directory has `DeviceProfile` that's not used. Should we:

**Option A**: Delete profiles module entirely
- Remove `src/profiles/` directory
- Keep useful parts (DataTypeConverter) elsewhere
- Clean up unused code

**Option B**: Keep and use DeviceProfile
- DeviceProfile already maps from Device config
- Use it in modbus_worker for register mappings
- More abstraction layer

**Option C**: Keep only DataTypeConverter
- Remove DeviceProfile and RegisterProfile
- Keep DataTypeConverter and helpers
- Minimal useful subset

## Scope Boundaries

**IN SCOPE**:
- Complete register type support (Coil, Discrete, Input, Holding)
- Data type parsing (U16, I16, U32, I32, F32, Bool)
- SignalGroup-based read/write operations
- Remove MoveTo operation handling
- Code restructuring for maintainability
- Remove unused code

**OUT OF SCOPE**:
- Changes to JSON-RPC API (rpc_worker)
- Changes to HTTP API (http_worker)
- Changes to configuration format
- RoboPLC framework updates
- Other worker modifications

## User Decisions

### Decision 1: Register Parsing Strategy
**CHOSEN: Option A** - Batch Read + Memory Parse
- Read all registers in SignalGroup at once (single Modbus operation)
- Parse fields in memory based on offset and data type configuration
- Most efficient for typical PLC scenarios where fields are consecutive
- Implementation: Read N registers → extract field values using DataTypeConverter

### Decision 2: Data Type Conversion
**CHOSEN: Option C** - Move to Shared Module
- Create `src/data_conversion.rs` module
- Move `DataTypeConverter`, `convert_byte_order`, and helpers from profiles
- Both modbus_worker and other modules use this shared module
- Single source of truth for data type conversion logic

### Decision 3: Register Type Support
**CHOSEN: Option A** - One Register Type Per Group
- Keep current SignalGroup design: one register type + consecutive addresses
- Create separate SignalGroups for different register types (Coils, Inputs, etc.)
- Simpler implementation, matches real-world PLC organization
- Validation: ensure all fields fit within register_count

### Decision 4: Write Operations API
**CHOSEN: Option C** - Both Approaches
- **Field-based API**: Accept `{"field_name": value, ...}` → parse and convert
- **Raw values API**: Accept `{"values": [100, 200, ...]}` → write directly
- Maximum flexibility for different use cases
- User-friendly for common case, powerful for advanced use

### Decision 5: Code Organization
**CHOSEN: Option B** - Module Split
- Create `src/workers/modbus/` directory structure:
  - `mod.rs` - Main ModbusWorker struct and Worker trait impl
  - `client.rs` - ModbusClient (connection management)
  - `operations.rs` - ModbusOp enum and operation handling
  - `parsing.rs` - Register parsing and data conversion
  - `types.rs` - Custom types (TransactionId, ConnectionState, etc.)
- Clean separation of concerns, easier to test and maintain

### Decision 6: Profiles Module
**CHOSEN: Option A** - Delete Profiles Module
- Remove `src/profiles/` directory entirely
- Keep useful parts: move DataTypeConverter to `src/data_conversion.rs`
- Clean up unused DeviceProfile and RegisterProfile code
- Simpler codebase, less abstraction layers

## Implementation Summary

### Key Components to Create/Modify

1. **src/data_conversion.rs** (NEW)
   - Move DataTypeConverter trait and DefaultDataTypeConverter impl
   - Move convert_byte_order, bytes_len helper functions
   - Move RegisterPair struct
   - Add comprehensive tests

2. **src/workers/modbus/** (NEW DIRECTORY)
   - Reorganize modbus_worker.rs into 5 files
   - Add support for all 4 Modbus register types
   - Implement batch read + field parsing
   - Implement dual write API (field-based + raw)

3. **src/workers/modbus/parsing.rs** (NEW)
   - Signal group field parser
   - Register value extraction by offset
   - Multi-register data type handling (U32, F32, etc.)
   - Byte order conversion

4. **src/workers/modbus/operations.rs** (NEW)
   - Enhanced ModbusOp enum with register type
   - Operation factory methods for each register type
   - Remove MoveTo operation

5. **src/profiles/** (DELETE)
   - Remove entire directory
   - Update imports in lib.rs and other files

### Changes to Existing Files

1. **src/workers/modbus_worker.rs** → DELETE after splitting
2. **src/lib.rs** - Update module imports, remove profiles
3. **src/messages.rs** - Remove MoveTo from Operation enum (optional, or just ignore it)
4. **config.rs** - No changes needed (SignalGroup design stays same)

### Architecture After Refactoring

```
src/
├── data_conversion.rs (NEW - shared conversion logic)
├── workers/
│   ├── modbus/ (NEW DIRECTORY)
│   │   ├── mod.rs (ModbusWorker main)
│   │   ├── client.rs (ModbusClient)
│   │   ├── operations.rs (ModbusOp, register operations)
│   │   ├── parsing.rs (field parsing, data extraction)
│   │   └── types.rs (TransactionId, ConnectionState, etc.)
│   ├── mod.rs (update to export modbus module)
│   └── ... (other workers unchanged)
└── profiles/ (DELETE ENTIRE DIRECTORY)
```