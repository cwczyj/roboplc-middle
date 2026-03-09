# roboplc-middleware

Communication middleware converting JSON-RPC to Modbus TCP for PLCs and robot arms.

## Overview

This project provides a RoboPLC-based middleware that:
- Exposes a JSON-RPC 2.0 API for device control
- Manages Modbus TCP connections to PLCs and robot arms
- Provides HTTP management endpoints
- Monitors device latency with 3-sigma anomaly detection
- Supports hot configuration reload
- Independent heartbeat monitoring with latency tracking

## Architecture

```
┌─────────────────┐     ┌──────────────────┐     ┌─────────────┐
│   JSON-RPC      │────▶│  Device Manager  │────▶│   Modbus    │
│   Server        │     │  (Hub Router)    │     │   Workers   │
│   (port 8080)   │     └──────────────────┘     │  (per device)│
└─────────────────┘              │               └─────────────┘
        │                        │                       │
        │                        ▼                       │
        │              ┌──────────────────┐             │
        └─────────────▶│  HTTP API       │◀────────────┘
                       │  (port 8081)    │
                       └──────────────────┘
                              │
        ┌─────────────────────┼─────────────────────┐
        │                     │                     │
        ▼                     ▼                     ▼
┌──────────────┐     ┌──────────────┐     ┌──────────────┐
│ ConfigLoader │     │ Heartbeat    │     │  Latency     │
│ (Hot Reload) │     │  Worker      │     │  Monitor     │
└──────────────┘     └──────────────┘     └──────────────┘
```

## Workers

| Worker | Port | Description |
|--------|------|-------------|
| **RpcWorker** | 8080 | JSON-RPC 2.0 server (async architecture) |
| **HttpWorker** | 8081 | HTTP management API |
| **DeviceManager** | - | Routes messages between workers via Hub |
| **ModbusWorker** | - | Modbus TCP client (one per device) with RT scheduling |
| **ConfigLoader** | - | Hot configuration reload via file watching |
| **HeartbeatWorker** | - | Independent heartbeat detection with latency tracking |
| **LatencyMonitor** | - | 3-sigma latency anomaly detection |

## Modbus Support

### Register Types

| Prefix | Type | Modbus Code |
|--------|------|-------------|
| `c` | Coil | 0x |
| `d` | Discrete Input | 1x |
| `i` | Input Register | 3x |
| `h` | Holding Register | 4x |

### Data Types

| Type | Description | Registers |
|------|-------------|-----------|
| `U16` | Unsigned 16-bit integer | 1 |
| `I16` | Signed 16-bit integer | 1 |
| `U32` | Unsigned 32-bit integer | 2 |
| `I32` | Signed 32-bit integer | 2 |
| `F32` | 32-bit floating point | 2 |
| `Bool` | Boolean | 1 |

## Configuration

Create `config.toml`:

```toml
[server]
rpc_port = 8080
http_port = 8081

[logging]
level = "info"
file = "/var/log/roboplc-middleware.log"
daily_rotation = true

[[devices]]
id = "plc-1"
type = "plc"
address = "192.168.1.100"
port = 502
unit_id = 1
addressing_mode = "zero_based"
byte_order = "big_endian"
tcp_nodelay = true
max_concurrent_ops = 3
heartbeat_interval_sec = 30

[[devices.signal_groups]]
name = "temperature_sensor"
description = "Temperature sensor data"
register_address = "h100"
register_count = 10

[[devices.signal_groups.fields]]
name = "temperature"
data_type = "F32"
offset = 0

[[devices.signal_groups.fields]]
name = "humidity"
data_type = "U16"
offset = 2
```

### Configuration Schema

#### [server]

| Field | Type | Required | Default | Description |
|-------|------|----------|---------|-------------|
| `rpc_port` | u16 | Yes | - | JSON-RPC server port |
| `http_port` | u16 | Yes | - | HTTP API port |

#### [logging]

| Field | Type | Required | Default | Description |
|-------|------|----------|---------|-------------|
| `level` | String | Yes | - | Log level: trace/debug/info/warn/error |
| `file` | String | Yes | - | Log file path |
| `daily_rotation` | bool | Yes | - | Enable daily log rotation |

#### [[devices]]

| Field | Type | Required | Default | Description |
|-------|------|----------|---------|-------------|
| `id` | String | Yes | - | Unique device identifier |
| `type` | String | No | "plc" | Device type: plc / robot_arm |
| `address` | String | Yes | - | Modbus TCP address |
| `port` | u16 | Yes | - | Modbus TCP port (usually 502) |
| `unit_id` | u8 | Yes | - | Modbus unit ID |
| `addressing_mode` | String | No | "zero_based" | Addressing: zero_based / one_based |
| `byte_order` | String | No | "big_endian" | Byte order: big_endian / little_endian / little_endian_byte_swap / mid_big |
| `tcp_nodelay` | bool | No | true | Enable TCP_NODELAY |
| `max_concurrent_ops` | u8 | No | 3 | Max concurrent operations |
| `heartbeat_interval_sec` | u32 | No | 30 | Heartbeat interval in seconds |

#### [[devices.signal_groups]]

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `name` | String | Yes | Signal group name (used in API) |
| `description` | String | No | Signal group description |
| `register_address` | String | Yes | Modbus address with prefix (e.g., "h100") |
| `register_count` | u16 | Yes | Number of registers in this group |
| `fields` | Array | Yes | Field mappings |

#### [[devices.signal_groups.fields]]

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `name` | String | Yes | Field name |
| `data_type` | String | Yes | Data type: U16/I16/U32/I32/F32/Bool |
| `offset` | u16 | Yes | Register offset within group |

## Build

```bash
cargo build --release
```

## Run

```bash
# Production mode
cargo run --release

# Development mode (skips RT scheduling)
ROBOPLC_SIMULATED=1 cargo run
```

## API Endpoints

### JSON-RPC 2.0 (port 8080)

| Method | Description |
|--------|-------------|
| `ping` | Health check |
| `get_version` | Get middleware version |
| `get_device_list` | List all devices |
| `get_status` | Get device status |
| `read_signal_group` | Read a signal group from device |
| `write_signal_group` | Write values to a signal group |

#### JSON-RPC Examples

**Ping:**
```json
{"m": "ping", "p": {}}
```

**Read Signal Group:**
```json
{"m": "read_signal_group", "p": {"device_id": "plc-1", "group_name": "temperature_sensor"}}
```

**Write Signal Group:**
```json
{"m": "write_signal_group", "p": {"device_id": "plc-1", "group_name": "actuators", "data": {"valve_1": true, "valve_2": false}}}
```

### HTTP API (port 8081)

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/api/devices` | GET | List all devices with status |
| `/api/devices/{id}/status` | GET | Get specific device status |
| `/api/health` | GET | System health check |
| `/api/config` | GET | Current configuration |
| `/api/config/reload` | POST | Returns success (reload via file watcher) |

#### Health Status Values

- `healthy`: All devices connected
- `degraded`: Some devices disconnected
- `unhealthy`: All devices disconnected or no devices

## Monitoring

### Latency Anomaly Detection

The LatencyMonitor uses 3-sigma algorithm to detect latency anomalies:
- Maintains a rolling window of 100 latency samples per device
- Calculates mean and standard deviation
- Flags latencies exceeding mean + 3 × standard deviation as anomalies
- Requires minimum 10 samples before detection begins

### Device Events

Device state changes are tracked:
- `Connected`: Device successfully connected
- `Disconnected`: Device connection lost
- `Reconnecting`: Device attempting to reconnect
- `Error`: Error occurred
- `HeartbeatMissed`: Heartbeat timeout

## Project Structure

```
src/
├── lib.rs              # Main library exports, shared state (Variables)
├── main.rs             # Entry point
├── config.rs           # Configuration parsing and validation
├── messages.rs         # Message types for worker communication
├── data_conversion.rs  # Data type conversion utilities
├── workers/
│   ├── mod.rs          # Worker module exports
│   ├── rpc_worker.rs   # JSON-RPC 2.0 server (async)
│   ├── http_worker.rs  # HTTP REST API server
│   ├── manager.rs      # Device manager (message router)
│   ├── heartbeat_worker.rs  # Heartbeat detection
│   ├── latency_monitor.rs   # Latency anomaly detection
│   ├── config_loader.rs     # Hot config reload
│   ├── config_updater.rs    # Config update handler
│   └── modbus/         # Modbus implementation
│       ├── mod.rs
│       ├── client.rs   # Modbus TCP client
│       ├── worker.rs   # ModbusWorker implementation
│       ├── operations.rs # Register operations
│       ├── parsing.rs  # Signal group encoding/decoding
│       └── types.rs    # Shared types (Backoff, ConnectionState, etc.)
```

## Key Dependencies

- `roboplc`: Real-time PLC framework (workers, Hub, comm)
- `serde`/`serde_json`: Serialization
- `tokio`: Async runtime
- `actix-web`: HTTP server
- `thiserror`: Error handling
- `tracing`: Structured logging

## License

MIT
