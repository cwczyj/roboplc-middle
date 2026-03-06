//! Functional tests for worker behavior using public APIs

use roboplc_middleware::config::{AddressingMode, ByteOrder, Device, DeviceType};
use roboplc_middleware::workers::modbus_worker::{ConnectionState, ModbusWorker, TransactionId};
use std::time::Duration;

fn test_device() -> Device {
    Device {
        id: "test-device".to_string(),
        device_type: DeviceType::Plc,
        address: "127.0.0.1".to_string(),
        port: 502,
        unit_id: 1,
        addressing_mode: AddressingMode::ZeroBased,
        byte_order: ByteOrder::BigEndian,
        tcp_nodelay: true,
        max_concurrent_ops: 3,
        heartbeat_interval_sec: 30,
        signal_groups: vec![],
    }
}

#[test]
fn worker_can_be_created() {
    let device = test_device();
    let _worker = ModbusWorker::new(device);
}

#[test]
fn transaction_ids_are_unique() {
    let id1 = TransactionId::new();
    let id2 = TransactionId::new();
    let id3 = TransactionId::new();

    assert_ne!(id1.id, id2.id);
    assert_ne!(id2.id, id3.id);
}

#[test]
fn transaction_id_has_timestamp() {
    let id = TransactionId::new();
    assert!(id.elapsed() < Duration::from_secs(1));
}

#[test]
fn connection_state_equality() {
    assert_eq!(ConnectionState::Disconnected, ConnectionState::Disconnected);
    assert_ne!(ConnectionState::Disconnected, ConnectionState::Connected);
    assert_ne!(ConnectionState::Connecting, ConnectionState::Connected);
}

#[test]
fn connection_state_variants() {
    let disconnected = ConnectionState::Disconnected;
    let connecting = ConnectionState::Connecting;
    let connected = ConnectionState::Connected;

    assert!(matches!(disconnected, ConnectionState::Disconnected));
    assert!(matches!(connecting, ConnectionState::Connecting));
    assert!(matches!(connected, ConnectionState::Connected));
}

#[test]
fn worker_with_different_configs() {
    let mut device = test_device();
    device.max_concurrent_ops = 5;
    device.heartbeat_interval_sec = 10;

    let _worker = ModbusWorker::new(device);
}

#[test]
fn worker_with_unreachable_address() {
    let mut device = test_device();
    device.address = "0.0.0.0".to_string();
    device.port = 1;

    let _worker = ModbusWorker::new(device);
}
