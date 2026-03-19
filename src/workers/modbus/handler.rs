//! DeviceControl message handler for ModbusWorker

use crate::config::Device;
use crate::messages::Operation;
use crate::{Message, Variables};
use roboplc::controller::prelude::*;
use serde_json::Value as JsonValue;

use super::{
    encode_fields_to_registers, parse_register_address,
    parse_signal_group_fields, ConnectionState, ModbusOp, RegisterType,
};
use crate::workers::modbus::state::ModbusWorkerState;

/// Handler for Modbus device control operations
pub struct DeviceControlHandler {
    state: ModbusWorkerState,
}

impl DeviceControlHandler {
    pub fn new(device: Device) -> Self {
        Self {
            state: ModbusWorkerState::new(device),
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

    /// Create ModbusOp for writing registers/coils
    fn create_write_op(
        reg_type: RegisterType,
        address: u16,
        registers: &[u16],
    ) -> ModbusOp {
        match reg_type {
            RegisterType::Coil => {
                let values: Vec<bool> = registers.iter().map(|&v| v != 0).collect();
                if values.len() == 1 {
                    ModbusOp::WriteSingleCoil {
                        address,
                        value: values[0],
                    }
                } else {
                    ModbusOp::WriteMultipleCoils { address, values }
                }
            }
            RegisterType::Holding => {
                if registers.len() == 1 {
                    ModbusOp::WriteSingle {
                        address,
                        value: registers[0],
                    }
                } else {
                    ModbusOp::WriteMultiple {
                        address,
                        values: registers.to_vec(),
                    }
                }
            }
            _ => unreachable!(),
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
        respond_to: Option<std::sync::mpsc::Sender<crate::messages::DeviceResponseData>>,
        context: &Context<Message, Variables>,
    ) {
        let send_response = |success: bool, data: JsonValue, error: Option<String>| {
            if let Some(ref sender) = respond_to {
                let _ = sender.send((success, data, error));
            } else {
                context.hub().send(Message::DeviceResponse {
                    device_id: device_id.clone(),
                    success,
                    data,
                    error,
                    correlation_id,
                });
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
            Operation::ReadSignalGroup => {
                self.handle_read_signal_group(&params, send_response, context);
            }
            Operation::WriteSignalGroup => {
                self.handle_write_signal_group(&params, send_response, context);
            }
        }
    }

    fn handle_read_signal_group<F>(&mut self, params: &JsonValue, mut send_response: F, context: &Context<Message, Variables>)
    where
        F: FnMut(bool, JsonValue, Option<String>),
    {
        let group_name = params
            .get("group_name")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        let group_data = self
            .resolve_signal_group(group_name)
            .map(|g| (g.fields.clone(), self.state.device().byte_order.clone()));

        if let Some(modbus_op) = self.operation_to_modbus_op(&Operation::ReadSignalGroup, params) {
            let result = self.state.execute_operation(context, &modbus_op);

            if result.success {
                if let Some(latency) = result.data.get("latency_us").and_then(|v| v.as_u64()) {
                    self.state.record_communication(context, latency);
                }
            }

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

            send_response(result.success, response_data, result.error);
        } else {
            send_response(
                false,
                JsonValue::Null,
                Some(format!("Invalid signal group: {}", group_name)),
            );
        }
    }

    fn handle_write_signal_group<F>(&mut self, params: &JsonValue, mut send_response: F, context: &Context<Message, Variables>)
    where
        F: FnMut(bool, JsonValue, Option<String>),
    {
        let group_name = params
            .get("group_name")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        // Clone group data to avoid borrow conflicts
        let group_data = self
            .resolve_signal_group(group_name)
            .map(|g| {
                (
                    g.name.clone(),
                    g.register_address.clone(),
                    g.register_count,
                    g.fields.clone(),
                )
            });

        let Some((name, register_address, register_count, fields)) = group_data else {
            send_response(
                false,
                JsonValue::Null,
                Some(format!("Invalid signal group: {}", group_name)),
            );
            return;
        };

        let (reg_type, base_addr) = match parse_register_address(&register_address) {
            Some(result) => result,
            None => {
                send_response(
                    false,
                    JsonValue::Null,
                    Some(format!("Invalid register address: {}", register_address)),
                );
                return;
            }
        };

        if let Err(e) = self.validate_writable(reg_type) {
            send_response(false, JsonValue::Null, Some(e.to_string()));
            return;
        }

        if let Some(fields_data) = params.get("data").and_then(|v| v.as_object()) {
            self.handle_field_based_write(
                fields_data,
                &fields,
                reg_type,
                base_addr,
                &name,
                send_response,
                context,
            );
        } else if let Some(_raw_values) = params.get("values").and_then(|v| v.as_array()) {
            self.handle_raw_values_write(params, register_count, &name, send_response, context);
        } else {
            send_response(
                false,
                JsonValue::Null,
                Some("Missing 'data' or 'values' parameter".to_string()),
            );
        }
    }

    fn handle_field_based_write<F>(
        &mut self,
        fields_data: &serde_json::Map<String, JsonValue>,
        fields: &[crate::config::FieldMapping],
        reg_type: RegisterType,
        base_addr: u16,
        group_name: &str,
        mut send_response: F,
        context: &Context<Message, Variables>,
    ) where
        F: FnMut(bool, JsonValue, Option<String>),
    {
        // Step 1: Validate completeness - all fields must be provided
        if let Err(missing_fields) = self.validate_field_completeness(fields_data, fields) {
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

        // Step 2: Encode all fields to registers in one batch
        let registers = match encode_fields_to_registers(
            fields_data,
            fields,
            // Calculate total register count from fields
            fields.iter()
                .map(|f| f.offset as u16 + f.data_type.required_registers() as u16)
                .max()
                .unwrap_or(1),
            self.state.device().byte_order.clone(),
        ) {
            Some(regs) => regs,
            None => {
                send_response(
                    false,
                    JsonValue::Null,
                    Some("Failed to encode fields to registers".to_string()),
                );
                return;
            }
        };

        // Step 3: Single batch write operation
        let modbus_op = Self::create_write_op(reg_type, base_addr, &registers);
        let result = self.state.execute_operation(context, &modbus_op);

        if result.success {
            if let Some(latency) = result.data.get("latency_us").and_then(|v| v.as_u64()) {
                self.state.record_communication(context, latency);
            }
        }

        send_response(
            result.success,
            serde_json::json!({
                "group_name": group_name,
                "result": result.data
            }),
            result.error,
        );
    }

    /// Validate that all required fields are provided in the request
    ///
    /// Returns Ok(()) if all fields are present, Err(Vec<&str>) with missing field names
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

    fn handle_raw_values_write<F>(
        &mut self,
        params: &JsonValue,
        _register_count: u16,
        group_name: &str,
        mut send_response: F,
        context: &Context<Message, Variables>,
    ) where
        F: FnMut(bool, JsonValue, Option<String>),
    {
        if let Some(modbus_op) = self.operation_to_modbus_op(&Operation::WriteSignalGroup, params) {
            let result = self.state.execute_operation(context, &modbus_op);

            if result.success {
                if let Some(latency) = result.data.get("latency_us").and_then(|v| v.as_u64()) {
                    self.state.record_communication(context, latency);
                }
            }
            send_response(
                result.success,
                serde_json::json!({
                    "group_name": group_name,
                    "result": result.data
                }),
                result.error,
            );
        } else {
            send_response(
                false,
                JsonValue::Null,
                Some(format!("Invalid signal group: {}", group_name)),
            );
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
}
