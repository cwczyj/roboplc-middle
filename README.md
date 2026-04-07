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

### Development Build

```bash
cargo build
```

### Production Build (Release)

```bash
cargo build --release
```

The release binary will be at `target/release/roboplc-middleware`.

## Run

### Development Mode

Development mode skips real-time scheduling requirements, no root privilege needed:

```bash
ROBOPLC_SIMULATED=1 cargo run
```

Or run the release binary:

```bash
ROBOPLC_SIMULATED=1 ./target/release/roboplc-middleware
```

### Production Mode

Production mode enables real-time FIFO scheduling for deterministic latency. **Root privilege is required**:

```bash
# Run with real-time scheduling (requires root)
sudo ./target/release/roboplc-middleware
```

### Configuration File

The middleware reads `config.toml` from the current working directory. You can:

```bash
# Option 1: Run from directory containing config.toml
cd /path/to/config/directory
sudo /path/to/roboplc-middleware

# Option 2: Create symlink
sudo ln -s /etc/roboplc/config.toml config.toml
sudo ./target/release/roboplc-middleware
```

## Production Deployment

### System Requirements

| Requirement | Minimum | Recommended |
|-------------|---------|-------------|
| OS | Linux (kernel 3.0+) | Linux (kernel 5.0+) |
| CPU | 2 cores | 4+ cores |
| Memory | 512 MB | 1 GB |
| Privileges | - | root (for RT scheduling) |

### Performance Tuning

The middleware is optimized for high-frequency multi-client access:

| Parameter | Location | Description |
|-----------|----------|-------------|
| `max_blocking_threads` | worker.rs | Concurrent request capacity (default: 128) |
| `mpsc channel capacity` | worker.rs | Request queue size (default: 5000) |
| `max_concurrent_ops` | config.toml | Per-device concurrent operations (default: 3) |

### Adjusting Concurrency

**Per-device concurrency** - in `config.toml`:

```toml
[[devices]]
id = "robot-arm-1"
# ... other settings ...
max_concurrent_ops = 5  # Allow 5 concurrent operations to this device
```

**Global request capacity** - rebuild after modifying `src/workers/rpc/worker.rs`:

```rust
.max_blocking_threads(128)  // Supports 64+ concurrent requests
let (tx, rx) = mpsc::channel::<DeviceControlRequest>(5000);  // Request queue
```

### Running as systemd Service

Create `/etc/systemd/system/roboplc-middleware.service`:

```ini
[Unit]
Description=RoboPLC Middleware
After=network.target

[Service]
Type=simple
User=root
WorkingDirectory=/opt/roboplc
ExecStart=/opt/roboplc/roboplc-middleware
Restart=always
RestartSec=5
LimitRTPRIO=99
LimitMEMLOCK=infinity

[Install]
WantedBy=multi-user.target
```

Enable and start:

```bash
sudo systemctl daemon-reload
sudo systemctl enable roboplc-middleware
sudo systemctl start roboplc-middleware
sudo systemctl status roboplc-middleware
```

### High-Frequency Access Support

The middleware supports multiple TCP clients accessing at ~50ms frequency:

- **Blocking thread pool**: 128 threads (supports 64+ concurrent requests)
- **Channel capacity**: 5000 queued requests
- **Per-device concurrency**: Configurable via `max_concurrent_ops`
- **Non-blocking queue**: Returns error immediately when at capacity

### Monitoring

Check logs:

```bash
# If using systemd
sudo journalctl -u roboplc-middleware -f

# Or check configured log file
tail -f /var/log/roboplc-middleware.log
```

Health check:

```bash
curl http://localhost:8081/api/health
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

## Streaming Data Mode

Streaming mode provides efficient, low-latency data access for high-frequency monitoring scenarios such as real-time robot arm position tracking. Instead of polling via JSON-RPC requests, clients subscribe to data streams and receive updates automatically.

### When to Use Streaming Mode

| Scenario | Recommended Approach |
|----------|---------------------|
| Real-time monitoring (>10Hz) | **Streaming mode** (SSE) |
| Periodic checks (<1Hz) | JSON-RPC polling |
| Control/Write operations | JSON-RPC |
| One-shot reads | HTTP Cache endpoint |

### Architecture

```
┌─────────────────┐     ┌──────────────────┐     ┌─────────────┐
│   DataStream    │────▶│   DataCache      │◀────│  SSE Client │
│   Worker        │     │   (Variables)    │     │  (Browser)  │
│  (polling)      │     └──────────────────┘     └─────────────┘
└─────────────────┘              │                       ▲
         │                       │                       │
         ▼                       ▼                       │
┌─────────────────┐     ┌──────────────────┐            │
│   Modbus        │     │   Hub Broadcast  │────────────┘
│   Worker        │     │   (SSE stream)   │
└─────────────────┘     └──────────────────┘
```

**Key components:**
- **DataStreamWorker**: Background polling at configured intervals
- **DataCache**: Thread-safe in-memory cache with LRU eviction
- **SSE Endpoint**: Server-sent events for real-time streaming
- **HTTP Cache Endpoint**: Direct cache queries for one-shot reads

### Configuration

Add `[[streams]]` sections to your `config.toml`:

```toml
# Stream robot arm position at 100Hz (10ms interval)
[[streams]]
device_id = "robot-arm-1"
signal_group = "position"
poll_interval_ms = 10
enabled = true

# Stream velocity at 50Hz (20ms interval)
[[streams]]
device_id = "robot-arm-1"
signal_group = "velocity"
poll_interval_ms = 20
enabled = true

# Optional: Global stream settings
[stream_settings]
max_streams_per_device = 10
default_poll_interval_ms = 100
cache_size = 1000
```

**Configuration options:**

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `device_id` | String | Required | Device identifier (must match [[devices]] id) |
| `signal_group` | String | Required | Signal group name to stream |
| `poll_interval_ms` | u64 | Required | Poll interval in milliseconds (5-5000) |
| `enabled` | bool | true | Enable/disable this stream |

### Streaming Endpoints

#### Server-Sent Events (SSE)

Endpoint: `GET /api/stream?device={device_id}&groups={group1,group2}`

Connects to an SSE stream that pushes data updates in real-time.

**Query parameters:**
- `device` (required): Device identifier
- `groups` (required): Comma-separated list of signal groups

**SSE message format:**
```
data: {"device_id":"robot-arm-1","signal_group":"position","values":{"x_position":100.5,"y_position":200.0,"z_position":50.0},"timestamp_ms":1234567890,"latency_us":2500}

```

**Features:**
- Automatic heartbeat (every 15 seconds when idle)
- Filtered by device and signal groups
- Multi-client support (shared polling worker)
- Automatic reconnection support

#### HTTP Cache Endpoint

Endpoint: `GET /api/cache/{device}/{group}`

Returns the latest cached value for a specific device and signal group.

**Response format:**
```json
{
  "device_id": "robot-arm-1",
  "signal_group": "position",
  "values": {
    "x_position": 100.5,
    "y_position": 200.0,
    "z_position": 50.0
  },
  "timestamp_ms": 1234567890,
  "cache_age_ms": 5,
  "fresh": true
}
```

**Status codes:**
- `200 OK`: Cache hit, returns cached data
- `404 Not Found`: Cache miss (no data available)

### Client Examples

#### Python (SSE Client)

```python
import json
import requests

def stream_device_data(device_id, groups):
    """Stream data from middleware using SSE."""
    url = f"http://localhost:8081/api/stream"
    params = {
        "device": device_id,
        "groups": ",".join(groups)
    }
    
    with requests.get(url, params=params, stream=True) as response:
        response.raise_for_status()
        
        for line in response.iter_lines():
            if not line:
                continue
                
            line = line.decode('utf-8')
            
            # Skip heartbeat comments
            if line.startswith(':'):
                continue
                
            # Parse data lines
            if line.startswith('data: '):
                data = json.loads(line[6:])
                print(f"[{data['timestamp_ms']}] {data['signal_group']}: {data['values']}")

# Stream position and velocity
stream_device_data("robot-arm-1", ["position", "velocity"])
```

#### JavaScript (EventSource)

```javascript
const deviceId = 'robot-arm-1';
const groups = ['position', 'velocity'];

const eventSource = new EventSource(
    `http://localhost:8081/api/stream?device=${deviceId}&groups=${groups.join(',')}`
);

eventSource.onmessage = (event) => {
    const data = JSON.parse(event.data);
    console.log(`[${data.timestamp_ms}] ${data.signal_group}:`, data.values);
};

eventSource.onerror = (error) => {
    console.error('SSE error:', error);
    // EventSource auto-reconnects on error
};

// Clean up on page unload
window.addEventListener('beforeunload', () => {
    eventSource.close();
});
```

#### curl (HTTP Cache)

```bash
# Query cached value for position
curl -s http://localhost:8081/api/cache/robot-arm-1/position | jq

# Output:
# {
#   "device_id": "robot-arm-1",
#   "signal_group": "position",
#   "values": { "x_position": 100.5, "y_position": 200.0, "z_position": 50.0 },
#   "timestamp_ms": 1234567890,
#   "cache_age_ms": 3,
#   "fresh": true
# }

# Follow SSE stream (requires curl with HTTP/1.1 support)
curl -N "http://localhost:8081/api/stream?device=robot-arm-1&groups=position"
```

### Performance

**Latency Comparison:**

| Access Method | Typical Latency | Use Case |
|---------------|-----------------|----------|
| JSON-RPC request-response | ~5ms + RTT | Control operations |
| HTTP Cache read | < 0.1ms | One-shot reads |
| SSE delivery | ~0.5ms | Real-time streaming |

**Throughput:**

| Metric | Value | Notes |
|--------|-------|-------|
| Max polling rate | 1000Hz (1ms) | With `poll_interval_ms = 1` |
| Recommended max | 100Hz (10ms) | Stable for most devices |
| Concurrent SSE clients | 1000+ | Tested with 10ms poll interval |
| Cache read latency (p99) | < 0.1ms | In-memory access |
| Memory per stream | ~2KB | Including cache entry overhead |

**Polling Strategy:**

The DataStreamWorker uses a hybrid polling strategy:
1. **Grouping**: Streams with the same `poll_interval_ms` are grouped together
2. **Parallel within group**: Group members are polled in parallel using `tokio::spawn`
3. **Serial between groups**: Groups execute sequentially to avoid overwhelming devices

**Tuning Tips:**

1. **Match poll interval to device capability**: Most PLCs handle 50-100Hz comfortably
2. **Group related signals**: Use the same `poll_interval_ms` for signals that update together
3. **Monitor latency**: Check logs for `DataStreamUpdate` latency warnings
4. **Cache sizing**: Default cache holds 100 entries; increase `cache_size` in `[stream_settings]` if needed

**Why No WebSocket?**

Server-Sent Events (SSE) was chosen over WebSocket because:
- **Simpler protocol**: HTTP-based, no upgrade handshake complexity
- **Automatic reconnection**: Built into EventSource API
- **Sufficient for our use case**: Data flows device → client only (no client → server streaming needed)
- **Better compatibility**: Works through most proxies and firewalls

For bidirectional communication or custom protocols, extend the existing JSON-RPC interface.

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
