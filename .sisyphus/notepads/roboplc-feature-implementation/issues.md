## Pre-existing Issues

### Compilation Error in manager.rs (2026-03-02)
**Location**: src/workers/manager.rs:51, 62

**Error**: Pattern matching for Message::DeviceControl and Message::DeviceResponse doesn't mention the `correlation_id` field.

**Details**:
```rust
// Line 51-55
Message::DeviceControl {
    device_id,
    operation,
    params: _,  // Missing correlation_id field
} => {

// Line 62-67
Message::DeviceResponse {
    device_id,
    success,
    data: _,
    error: _,  // Missing correlation_id field
} => {
```

**Fix**: Add `correlation_id: _` or use `..` wildcard to ignore missing fields.

**Impact**: Blocks cargo check from completing, but unrelated to api.rs changes.

**Priority**: Medium - Should be fixed before integrating api.rs into main build
