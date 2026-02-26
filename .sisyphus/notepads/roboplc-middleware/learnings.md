# Learnings

## 2026-02-26 Task 1: Cargo.toml Setup

### RoboPLC Dependencies
- roboplc version 0.6.4 is current
- Use `default-features = false` and explicitly enable needed features:
  - `modbus` for Modbus TCP/RTU support
  - `locking-rt-safe` for real-time safe locking (Linux only, Kernel 5.14+)
- roboplc-rpc version 0.1.8 for JSON-RPC 2.0
  - Enable `std` feature for standard library support

### Edition
- Use edition = "2021" (not 2024 - Rust 2024 edition is not stable)

### Profile Settings
- `opt-level = 3` for release builds
- `lto = "thin"` for link-time optimization
- `codegen-units = 1` for better optimization
- `strip = true` to reduce binary size

### Dependency Groups
- Framework: roboplc, roboplc-rpc
- Serialization: serde, serde_json
- Config: toml
- Logging: tracing, tracing-subscriber, tracing-appender
- File watching: notify
- Async: tokio
- Locking: parking_lot_rt
- Error: thiserror, anyhow
- Time: chrono

### System Requirements
- Linker `cc` required for compilation
- Real-time locking features require Linux Kernel 5.14+