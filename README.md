# roboplc-middleware

Communication middleware converting JSON-RPC to Modbus TCP for PLCs and robot arms.

## Overview

This project provides a RoboPLC-based middleware that:
- Exposes a JSON-RPC 2.0 API for device control
- Manages Modbus TCP connections to PLCs and robot arms
- Provides HTTP management endpoints
- Monitors device latency with 3-sigma anomaly detection
- Supports hot configuration reload

## Architecture

```
┌─────────────────┐     ┌──────────────────┐     ┌─────────────┐
│   JSON-RPC      │────▶│  Device Manager  │────▶│   Modbus    │
│   Server        │     │  (Hub Router)    │     │   Workers   │
└─────────────────┘     └──────────────────┘     └─────────────┘
        │                        │                       │
        │                        ▼                       │
        │              ┌──────────────────┐             │
        └─────────────▶│  HTTP API       │◀────────────┘
                       │  /api/devices   │
                       │  /api/health    │
                       └──────────────────┘
```

## Workers

- **RpcWorker**: JSON-RPC 2.0 server (port 8080)
- **DeviceManager**: Routes messages between workers via Hub
- **ModbusWorker**: Modbus TCP client with RT scheduling, exponential backoff, connection pooling
- **HttpWorker**: HTTP management API (port 8081)
- **ConfigLoader**: Hot configuration reload with file watching
- **LatencyMonitor**: 3-sigma latency anomaly detection

## Configuration

Create `config.toml`:

```toml
[server]
rpc_port = 8080
http_port = 8081

[[devices]]
id = "plc-1"
address = "192.168.1.100"
port = 502
unit_id = 1
addressing_mode = "zero_based"
byte_order = "big_endian"
heartbeat_interval_sec = 5
max_concurrent_ops = 3

[[devices.register_mappings]]
signal_name = "temperature"
address = "h100"
data_type = "U16"

[[devices.register_mappings]]
signal_name = "pressure"
address = "h101"
data_type = "F32"
```

### Configuration Schema

| Field | Type | Description |
|-------|------|-------------|
| `server.rpc_port` | u16 | JSON-RPC server port |
| `server.http_port` | u16 | HTTP API port |
| `devices[].id` | String | Unique device identifier |
| `devices[].address` | String | Modbus TCP address |
| `devices[].port` | u16 | Modbus TCP port |
| `devices[].unit_id` | u8 | Modbus unit ID |
| `devices[].addressing_mode` | String | "zero_based" or "one_based" |
| `devices[].byte_order` | String | "big_endian", "little_endian", etc. |
| `devices[].heartbeat_interval_sec` | u64 | Heartbeat interval |
| `devices[].max_concurrent_ops` | usize | Max concurrent operations |

### Register Address Format

| Prefix | Register Type |
|--------|--------------|
| `c` | Coil (0x) |
| `d` | Discrete Input (1x) |
| `i` | Input Register (3x) |
| `h` | Holding Register (4x) |

Example: `h100` = Holding Register at address 100

## Build

```bash
cargo build --release
```

## Run

```bash
cargo run
```

For development (skips RT scheduling):
```bash
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
| `set_register` | Write Modbus register |
| `get_register` | Read Modbus register |
| `move_to` | Move robot arm |
| `read_batch` | Read multiple registers |
| `write_batch` | Write multiple registers |

### HTTP API (port 8081)

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/api/devices` | GET | List all devices with status |
| `/api/devices/{id}/status` | GET | Get specific device status |
| `/api/health` | GET | Health check |
| `/api/config` | GET | Current configuration |
| `/api/config/reload` | POST | Reload configuration |

## Features

### Latency Monitoring

The middleware monitors device latency using 3-sigma anomaly detection:
- Tracks latency samples in a rolling window
- Calculates mean and standard deviation
- Flags anomalies when latency exceeds 3 standard deviations

### Connection Management

- Automatic reconnection with exponential backoff
- Jitter to prevent thundering herd
- Transaction ID tracking for request/response matching
- Connection state tracking (Connected/Disconnected/Reconnecting)

### Hot Reload

Configuration changes are detected automatically:
- File watching via notify crate
- Diff detection to avoid unnecessary reloads
- ConfigUpdate messages broadcast to workers

## Testing

```bash
cargo test
```

Current test coverage: 35 tests

## License

Apache-2.0