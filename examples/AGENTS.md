# Examples

Demo files and example configurations for roboplc-middleware.

## Directory Structure

```
examples/
├── frontend_demo.html          # SSE real-time joint monitor
├── config-basic.toml           # Basic config template
├── config-multi-device.toml    # Multi-device config template
└── upper_computer_examples/
    └── jsonrpc_tcp_json_examples.md  # JSON-RPC client examples
```

## frontend_demo.html

Real-time SSE frontend for monitoring robot arm joint positions.

### Usage

1. Start middleware: `cargo run`
2. Open `frontend_demo.html` in browser
3. Monitor J1-J6 joint positions in real-time

### Configuration

Hardcoded values (modify for your setup):
- Device ID: `Test-Dobot`
- Signal group: `real_time_joint_position`
- SSE endpoint: `http://localhost:8081/api/stream`

### Features

| Feature | Implementation |
|---------|----------------|
| SSE connection | Native `EventSource` API |
| Auto-reconnect | Built-in EventSource behavior |
| Joint display | J1-J6 grid with deg units (rad→deg conversion) |
| Status indicator | Visual connection state |
| Event log | Debug messages (max 50 entries) |
| Metadata | timestamp_ms, latency_us |

### SSE Data Format

```json
{
  "device_id": "Test-Dobot",
  "signal_group": "real_time_joint_position",
  "values": {"J1": 0.0, "J2": 0.0, ...},
  "timestamp_ms": 1234567890,
  "latency_us": 2500
}
```

### Customization

To monitor different device/group:

```javascript
// Edit line 341 in frontend_demo.html
const SSE_ENDPOINT = 'http://localhost:8081/api/stream?device=YOUR_DEVICE&groups=YOUR_GROUP';
```

## Config Templates

### config-basic.toml

Single device configuration template.

### config-multi-device.toml

Multiple devices with different signal groups.

## upper_computer_examples/

JSON-RPC client integration examples for upper computer systems.

### jsonrpc_tcp_json_examples.md

Protocol documentation and client implementation patterns.