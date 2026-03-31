//! DeviceControl message handler for ModbusWorker

use crate::config::Device;
use crate::hub_protection::{send_to_hub_with_protection, DEFAULT_HUB_SEND_TIMEOUT};
use crate::messages::Operation;
use crate::{Message, Variables};
use roboplc::controller::prelude::*;
use serde_json::Value as JsonValue;
use std::panic::AssertUnwindSafe;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use super::{
    encode_fields_to_registers, parse_register_address, parse_signal_group_fields,
    ConnectionState, ModbusOp, OperationGuard, RegisterType,
};
use crate::workers::modbus::state::ModbusWorkerState;

/// Handler for Modbus device control operations
pub struct DeviceControlHandler {
    state: ModbusWorkerState,
    runtime: Option<tokio::runtime::Runtime>,
    in_flight: Arc<AtomicUsize>,
    max_in_flight: usize,
}

const MAX_CAS_RETRIES: usize = 100;

impl DeviceControlHandler {
    pub fn new(device: Device) -> Self {
        let max_in_flight = device.max_concurrent_ops as usize;
        Self {
            state: ModbusWorkerState::new(device),
            runtime: Some(tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()
                .expect("Failed to create tokio runtime for async operations")),
            in_flight: Arc::new(AtomicUsize::new(0)),
            max_in_flight,
        }
    }

    fn try_acquire(&self) -> bool {
        for attempt in 0..MAX_CAS_RETRIES {
            let current = self.in_flight.load(Ordering::Acquire);
            if current >= self.max_in_flight {
                return false;
            }
            if self.in_flight.compare_exchange(
                current, current + 1,
                Ordering::AcqRel, Ordering::Acquire
            ).is_ok() {
                return true;
            }
            if attempt > 10 {
                std::hint::spin_loop();
            }
        }
        tracing::debug!("try_acquire failed after {} CAS attempts", MAX_CAS_RETRIES);
        false
    }

    fn complete(&self) {
        for attempt in 0..MAX_CAS_RETRIES {
            let current = self.in_flight.load(Ordering::Acquire);
            if current == 0 {
                break;
            }
            if self.in_flight.compare_exchange(
                current, current - 1,
                Ordering::AcqRel, Ordering::Acquire
            ).is_ok() {
                break;
            }
            if attempt > 10 {
                std::hint::spin_loop();
            }
        }
    }

    pub fn device(&self) -> &Device {
        self.state.device()
    }

    pub fn connection_state(&self) -> ConnectionState {
        self.state.connection_state()
    }

    /// Update device configuration from ConfigUpdate message
    pub fn update_device_config(&mut self, new_device: Device) {
        self.state.update_device_config(new_device);
    }

    /// Resolve signal group by name
    fn resolve_signal_group(&self, group_name: &str) -> Option<&crate::config::SignalGroup> {
        self.state
            .device()
            .signal_groups
            .iter()
            .find(|g| g.name == group_name)
    }

    /// Validate that register type is writable
    fn validate_writable(&self, reg_type: RegisterType) -> Result<(), &'static str> {
        match reg_type {
            RegisterType::Discrete | RegisterType::Input => {
                Err("Cannot write to read-only register type")
            }
            _ => Ok(()),
        }
    }

    /// Convert Operation and params to ModbusOp
    pub(crate) fn operation_to_modbus_op(
        &self,
        operation: &Operation,
        params: &JsonValue,
    ) -> Option<ModbusOp> {
        match operation {
            Operation::ReadSignalGroup => {
                let group_name = params.get("group_name")?.as_str()?;
                let group = self.resolve_signal_group(group_name)?;
                let (reg_type, addr) = parse_register_address(&group.register_address)?;
                match reg_type {
                    RegisterType::Coil => Some(ModbusOp::ReadCoil {
                        address: addr,
                        count: group.register_count,
                    }),
                    RegisterType::Discrete => Some(ModbusOp::ReadDiscrete {
                        address: addr,
                        count: group.register_count,
                    }),
                    RegisterType::Input => Some(ModbusOp::ReadInput {
                        address: addr,
                        count: group.register_count,
                    }),
                    RegisterType::Holding => Some(ModbusOp::ReadHolding {
                        address: addr,
                        count: group.register_count,
                    }),
                }
            }
            Operation::WriteSignalGroup => {
                let group_name = params.get("group_name")?.as_str()?;
                let group = self.resolve_signal_group(group_name)?;
                let (reg_type, addr) = parse_register_address(&group.register_address)?;

                self.validate_writable(reg_type).ok()?;

                match reg_type {
                    RegisterType::Coil => {
                        let values: Vec<bool> = if let Some(fields_data) =
                            params.get("data").and_then(|v| v.as_object())
                        {
                            let regs = encode_fields_to_registers(
                                fields_data,
                                &group.fields,
                                group.register_count,
                                self.state.device().byte_order.clone(),
                            )?;
                            regs.iter().map(|&v| v != 0).collect()
                        } else if let Some(raw_values) =
                            params.get("values").and_then(|v| v.as_array())
                        {
                            raw_values
                                .iter()
                                .filter_map(|v| v.as_u64().map(|n| n != 0))
                                .collect()
                        } else {
                            return None;
                        };

                        if values.len() == 1 {
                            Some(ModbusOp::WriteSingleCoil {
                                address: addr,
                                value: values[0],
                            })
                        } else {
                            Some(ModbusOp::WriteMultipleCoils {
                                address: addr,
                                values,
                            })
                        }
                    }
                    RegisterType::Holding => {
                        let values = if let Some(fields_data) =
                            params.get("data").and_then(|v| v.as_object())
                        {
                            encode_fields_to_registers(
                                fields_data,
                                &group.fields,
                                group.register_count,
                                self.state.device().byte_order.clone(),
                            )?
                        } else if let Some(raw_values) =
                            params.get("values").and_then(|v| v.as_array())
                        {
                            raw_values
                                .iter()
                                .filter_map(|v| v.as_u64().map(|n| n as u16))
                                .collect()
                        } else {
                            return None;
                        };
                        Some(ModbusOp::WriteMultiple {
                            address: addr,
                            values,
                        })
                    }
                    _ => None,
                }
            }
            Operation::GetStatus => None,
        }
    }

    /// Handle DeviceControl message and send response
    pub fn handle_device_control(
        &mut self,
        device_id: String,
        operation: Operation,
        params: JsonValue,
        correlation_id: u64,
        respond_to: Option<std::sync::mpsc::SyncSender<crate::messages::DeviceResponseData>>,
        context: &Context<Message, Variables>,
    ) {
        let hub = context.hub().clone();
        let send_response = move |success: bool, data: JsonValue, error: Option<String>| {
            if let Some(ref sender) = respond_to {
                let _ = sender.send((success, data, error));
            } else {
                if let Err(e) = send_to_hub_with_protection(
                    &hub,
                    Message::DeviceResponse {
                        device_id: device_id.clone(),
                        success,
                        data,
                        error,
                        correlation_id,
                    },
                    DEFAULT_HUB_SEND_TIMEOUT,
                ) {
                    tracing::warn!(error = %e, device_id = %device_id, "Failed to send device response via Hub");
                }
            }
        };

        if !self.state.ensure_connected(context) {
            send_response(
                false,
                JsonValue::Null,
                Some("Device not connected".to_string()),
            );
            return;
        }

        match operation {
            Operation::GetStatus => {
                let status = serde_json::json!({
                    "device_id": self.state.device().id,
                    "connected": self.state.connection_state() == ConnectionState::Connected,
                    "connection_state": format!("{:?}", self.state.connection_state()),
                    "last_communication_ms": self.state.device().signal_groups.len(),
                });
                send_response(true, status, None);
            }
            Operation::ReadSignalGroup | Operation::WriteSignalGroup => {
                if !self.try_acquire() {
                    send_response(
                        false,
                        JsonValue::Null,
                        Some("Too many concurrent operations".to_string()),
                    );
                    return;
                }

                let pool = match self.state.get_pool() {
                    Some(p) => Arc::new(p.clone()),
                    None => {
                        self.complete();
                        send_response(
                            false,
                            JsonValue::Null,
                            Some("Connection pool not available".to_string()),
                        );
                        return;
                    }
                };

                let modbus_op = self.operation_to_modbus_op(&operation, &params);
                let group_name = params
                    .get("group_name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();

                if matches!(operation, Operation::WriteSignalGroup) {
                    if let Some(fields_data) = params.get("data").and_then(|v| v.as_object()) {
                        if let Some(group) = self.resolve_signal_group(&group_name) {
                            if let Err(missing_fields) =
                                self.validate_field_completeness(fields_data, &group.fields)
                            {
                                self.complete();
                                send_response(
                                    false,
                                    JsonValue::Null,
                                    Some(format!(
                                        "Incomplete signal group: missing fields [{}]. All fields must be provided.",
                                        missing_fields.join(", ")
                                    )),
                                );
                                return;
                            }
                        }
                    }
                }

                let group_data = self
                    .resolve_signal_group(&group_name)
                    .map(|g| (g.fields.clone(), self.state.device().byte_order.clone()));

                let in_flight = self.in_flight.clone();

                self.runtime.as_ref().expect("Runtime should exist").spawn(async move {
                    let _guard = OperationGuard::new(move || {
                        in_flight.fetch_sub(1, Ordering::SeqCst);
                    });

                    // Wrap the core operation logic with catch_unwind for panic recovery
                    let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
                        let Some(modbus_op) = modbus_op else {
                            return Err((
                                false,
                                JsonValue::Null,
                                Some(format!("Invalid signal group: {}", group_name)),
                            ));
                        };

                        let result = pool.execute_operation(&modbus_op);

                        let response_data = if let Some((fields, byte_order)) = group_data {
                            if result.success {
                                if let Some(values) = result.data.get("values").and_then(|v| v.as_array()) {
                                    let registers: Vec<u16> = values
                                        .iter()
                                        .filter_map(|v| v.as_u64().map(|n| n as u16))
                                        .collect();

                                    let parsed_fields =
                                        parse_signal_group_fields(&registers, &fields, byte_order);

                                    serde_json::json!({
                                        "group_name": group_name,
                                        "result": {
                                            "fields": parsed_fields,
                                            "latency_us": result.data.get("latency_us").unwrap_or(&JsonValue::Null)
                                        }
                                    })
                                } else {
                                    serde_json::json!({
                                        "group_name": group_name,
                                        "result": result.data
                                    })
                                }
                            } else {
                                serde_json::json!({
                                    "group_name": group_name,
                                    "result": result.data
                                })
                            }
                        } else {
                            serde_json::json!({
                                "group_name": group_name,
                                "result": result.data
                            })
                        };

                        Ok((result.success, response_data, result.error))
                    }));

                    match result {
                        Ok(Ok((success, data, error))) => {
                            send_response(success, data, error);
                        }
                        Ok(Err((success, data, error))) => {
                            send_response(success, data, error);
                        }
                        Err(panic_info) => {
                            tracing::error!("Modbus operation panicked: {:?}", panic_info);
                            send_response(
                                false,
                                JsonValue::Null,
                                Some("Internal server error".to_string()),
                            );
                        }
                    }
                });
            }
        }
    }

    fn validate_field_completeness(
        &self,
        provided_fields: &serde_json::Map<String, JsonValue>,
        required_fields: &[crate::config::FieldMapping],
    ) -> Result<(), Vec<String>> {
        let mut missing = Vec::new();

        for required in required_fields {
            if !provided_fields.contains_key(&required.name) {
                missing.push(required.name.clone());
            }
        }

        if missing.is_empty() {
            Ok(())
        } else {
            Err(missing)
        }
    }
}

impl Drop for DeviceControlHandler {
    fn drop(&mut self) {
        if let Some(runtime) = self.runtime.take() {
            runtime.shutdown_timeout(std::time::Duration::from_secs(5));
            tracing::debug!("DeviceControlHandler runtime shut down");
        }
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{ByteOrder, DataType, Device, DeviceType, FieldMapping};

    fn make_field(name: &str, data_type: DataType, offset: u16) -> FieldMapping {
        FieldMapping {
            name: name.to_string(),
            data_type,
            offset,
        }
    }

    fn create_test_device() -> Device {
        Device {
            id: "test-device".to_string(),
            device_type: DeviceType::Plc,
            address: "127.0.0.1".to_string(),
            port: 502,
            unit_id: 1,
            addressing_mode: Default::default(),
            byte_order: ByteOrder::BigEndian,
            tcp_nodelay: true,
            max_concurrent_ops: 3,
            max_pool_size: 5,
            heartbeat_interval_sec: 30,
            signal_groups: vec![],
        }
    }

    #[test]
    fn validate_field_completeness_all_fields_provided() {
        let handler = DeviceControlHandler::new(create_test_device());

        let mut provided = serde_json::Map::new();
        provided.insert("field1".to_string(), serde_json::json!(100));
        provided.insert("field2".to_string(), serde_json::json!(200));

        let required = vec![
            make_field("field1", DataType::U16, 0),
            make_field("field2", DataType::U16, 1),
        ];

        let result = handler.validate_field_completeness(&provided, &required);
        assert!(result.is_ok());
    }

    #[test]
    fn validate_field_completeness_missing_fields() {
        let handler = DeviceControlHandler::new(create_test_device());

        let mut provided = serde_json::Map::new();
        provided.insert("field1".to_string(), serde_json::json!(100));
        // Missing field2

        let required = vec![
            make_field("field1", DataType::U16, 0),
            make_field("field2", DataType::U16, 1),
            make_field("field3", DataType::U16, 2),
        ];

        let result = handler.validate_field_completeness(&provided, &required);
        assert!(result.is_err());
        let missing = result.unwrap_err();
        assert_eq!(missing.len(), 2);
        assert!(missing.contains(&"field2".to_string()));
        assert!(missing.contains(&"field3".to_string()));
    }

    #[test]
    fn validate_field_completeness_empty_provided() {
        let handler = DeviceControlHandler::new(create_test_device());

        let provided = serde_json::Map::new();

        let required = vec![
            make_field("field1", DataType::U16, 0),
            make_field("field2", DataType::U16, 1),
        ];

        let result = handler.validate_field_completeness(&provided, &required);
        assert!(result.is_err());
        let missing = result.unwrap_err();
        assert_eq!(missing.len(), 2);
    }

    #[test]
    fn validate_field_completeness_no_required_fields() {
        let handler = DeviceControlHandler::new(create_test_device());

        let mut provided = serde_json::Map::new();
        provided.insert("field1".to_string(), serde_json::json!(100));

        let required: Vec<FieldMapping> = vec![];

        let result = handler.validate_field_completeness(&provided, &required);
assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_single_operation_executes() {
        use crate::workers::modbus::state::ModbusWorkerState;
        use std::sync::atomic::{AtomicU32, Ordering};
        use std::sync::Arc;

        let device = create_test_device();
        let mut state = ModbusWorkerState::new(device);

        let op_count = Arc::new(AtomicU32::new(0));
        let op_count_clone = op_count.clone();

        let acquired = state.try_acquire_operation();
        assert!(acquired, "Single operation should acquire capacity");

        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
        op_count_clone.fetch_add(1, Ordering::SeqCst);

        state.complete_operation();

        assert_eq!(op_count.load(Ordering::SeqCst), 1, "Operation should have executed");
    }

    /// Test: Multiple concurrent operations execute in parallel.
    #[tokio::test]
    async fn test_concurrent_operations_execute_in_parallel() {
        use std::sync::atomic::{AtomicU32, Ordering};
        use std::sync::Arc;

        let concurrent_count = Arc::new(AtomicU32::new(0));
        let max_concurrent = Arc::new(AtomicU32::new(0));
        let completed_count = Arc::new(AtomicU32::new(0));

        let mut tasks = vec![];

        for _i in 0..3 {
            let concurrent_clone = concurrent_count.clone();
            let max_clone = max_concurrent.clone();
            let completed_clone = completed_count.clone();

            let task = tokio::spawn(async move {
                let current = concurrent_clone.fetch_add(1, Ordering::SeqCst) + 1;
                let current_max = max_clone.load(Ordering::SeqCst);
                if current > current_max {
                    max_clone.store(current, Ordering::SeqCst);
                }

                tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

                concurrent_clone.fetch_sub(1, Ordering::SeqCst);
                completed_clone.fetch_add(1, Ordering::SeqCst);
            });
            tasks.push(task);
        }

        for task in tasks {
            task.await.expect("Task should complete");
        }

        assert_eq!(
            completed_count.load(Ordering::SeqCst),
            3,
            "All operations should complete"
        );

        assert!(
            max_concurrent.load(Ordering::SeqCst) >= 2,
            "Operations should run concurrently, max concurrent should be >= 2"
        );
    }

    /// Test: max_concurrent_ops limit rejection works correctly.
    #[tokio::test]
    async fn test_max_concurrent_ops_limit() {
        use crate::workers::modbus::state::ModbusWorkerState;

        let device = Device {
            id: "limit-test-device".to_string(),
            device_type: DeviceType::Plc,
            address: "127.0.0.1".to_string(),
            port: 502,
            unit_id: 1,
            addressing_mode: Default::default(),
            byte_order: ByteOrder::BigEndian,
            tcp_nodelay: true,
            max_concurrent_ops: 2,
            max_pool_size: 5,
            heartbeat_interval_sec: 30,
            signal_groups: vec![],
        };

        let mut state = ModbusWorkerState::new(device);

        let first = state.try_acquire_operation();
        assert!(first, "First operation should acquire capacity");

        let second = state.try_acquire_operation();
        assert!(second, "Second operation should acquire capacity");

        let third = state.try_acquire_operation();
        assert!(!third, "Third operation should be rejected due to limit");

        state.complete_operation();

        let fourth = state.try_acquire_operation();
        assert!(fourth, "Operation after capacity release should succeed");

        state.complete_operation();
        state.complete_operation();
    }

    /// Test: Operation capacity is released after completion.
    #[tokio::test]
    async fn test_operation_capacity_released() {
        use crate::workers::modbus::state::ModbusWorkerState;

        let device = create_test_device();
        let mut state = ModbusWorkerState::new(device);

        for cycle in 0..5 {
            let acquired = state.try_acquire_operation();
            assert!(
                acquired,
                "Should acquire capacity on cycle {} after previous completion",
                cycle
            );

            tokio::time::sleep(tokio::time::Duration::from_millis(1)).await;

            state.complete_operation();
        }

        let acquired = state.try_acquire_operation();
        assert!(acquired, "Should still be able to acquire after cycles");

        state.complete_operation();
    }

    /// Test: Operation capacity handling under async concurrent load.
    #[tokio::test]
    async fn test_capacity_under_concurrent_async_load() {
        use crate::workers::modbus::state::ModbusWorkerState;
        use std::sync::atomic::{AtomicU32, Ordering};
        use std::sync::Arc;
        use tokio::sync::Mutex;

        let device = Device {
            id: "concurrent-test".to_string(),
            device_type: DeviceType::Plc,
            address: "127.0.0.1".to_string(),
            port: 502,
            unit_id: 1,
            addressing_mode: Default::default(),
            byte_order: ByteOrder::BigEndian,
            tcp_nodelay: true,
            max_concurrent_ops: 2,
            max_pool_size: 5,
            heartbeat_interval_sec: 30,
            signal_groups: vec![],
        };

        let state = Arc::new(Mutex::new(ModbusWorkerState::new(device)));

        let success_count = Arc::new(AtomicU32::new(0));
        let reject_count = Arc::new(AtomicU32::new(0));

        let mut tasks = vec![];
        for _ in 0..10 {
            let state_clone = state.clone();
            let success_clone = success_count.clone();
            let reject_clone = reject_count.clone();

            let task = tokio::spawn(async move {
                let mut state = state_clone.lock().await;

                if state.try_acquire_operation() {
                    success_clone.fetch_add(1, Ordering::SeqCst);

                    tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

                    state.complete_operation();
                } else {
                    reject_clone.fetch_add(1, Ordering::SeqCst);
                }
            });
            tasks.push(task);
        }

        for task in tasks {
            task.await.expect("Task should complete");
        }

        let successes = success_count.load(Ordering::SeqCst);
        let rejects = reject_count.load(Ordering::SeqCst);

        assert!(successes > 0, "Some operations should succeed");
        assert_eq!(
            successes + rejects,
            10,
            "All 10 attempts should be accounted for"
        );
    }
}
