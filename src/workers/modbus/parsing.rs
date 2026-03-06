//! Modbus signal group field parsing
//!
//! This module provides parsing utilities for extracting field values from
//! Modbus register data using batch read + memory parse strategy.
//!
//! ## Strategy
//!
//! 1. Batch read all registers from device in one operation
//! 2. Parse individual fields from the register buffer in memory
//!
//! This approach minimizes device communication overhead.

use crate::config::{ByteOrder, DataType, FieldMapping};
use crate::data_conversion::DataTypeConverter;

/// Parsed field value from a SignalGroup
#[derive(Debug, Clone, PartialEq)]
pub struct ParsedField {
    /// Field name from configuration
    pub name: String,
    /// Parsed numeric value
    pub value: f64,
    /// Data type used for parsing
    pub data_type: DataType,
}

/// Parse SignalGroup fields from register values
///
/// This function implements the batch read + memory parse strategy:
/// - All registers are read in one operation (done by caller)
/// - Fields are parsed from the register buffer based on their offsets
///
/// # Arguments
///
/// * `registers` - Raw register values from Modbus device
/// * `fields` - Field mappings defining how to parse each field
/// * `byte_order` - Byte order for multi-register types
///
/// # Returns
///
/// Vector of `ParsedField` with extracted values. Fields with invalid
/// offsets or conversion failures are skipped.
///
/// # Examples
///
/// ```
/// use crate::config::{ByteOrder, DataType, FieldMapping};
/// use crate::workers::modbus::parsing::parse_signal_group_fields;
///
/// // Registers read from device
/// let registers = vec![0x1234, 0x5678];
///
/// // Field mapping: U16 at offset 0, F32 at offset 0 (uses 2 registers)
/// let fields = vec![
///     FieldMapping {
///         name: "value_u16".to_string(),
///         data_type: DataType::U16,
///         offset: 0,
///     },
/// ];
///
/// let parsed = parse_signal_group_fields(&registers, &fields, ByteOrder::BigEndian);
/// assert_eq!(parsed.len(), 1);
/// ```
pub fn parse_signal_group_fields(
    registers: &[u16],
    fields: &[FieldMapping],
    byte_order: ByteOrder,
) -> Vec<ParsedField> {
    let mut results = Vec::with_capacity(fields.len());

    for field in fields {
        // Calculate byte offset (each register is 2 bytes)
        let _byte_offset = (field.offset as usize) * 2;

        // Get required byte count for this data type
        let byte_count = match field.data_type {
            DataType::Bool => 1,
            DataType::U16 | DataType::I16 => 2,
            DataType::U32 | DataType::I32 | DataType::F32 => 4,
        };

        // Check if we have enough registers for this field
        let required_registers = match field.data_type {
            DataType::U16 | DataType::I16 | DataType::Bool => 1,
            DataType::U32 | DataType::I32 | DataType::F32 => 2,
        };

        if (field.offset as usize) + required_registers > registers.len() {
            // Not enough registers, skip this field
            continue;
        }

        // Extract bytes from registers
        let bytes = extract_bytes_from_registers(registers, field.offset as usize, byte_count);

        // Convert bytes to f64 using DataTypeConverter
        if let Some(value) =
            <crate::data_conversion::DefaultDataTypeConverter as DataTypeConverter>::from_bytes(
                &bytes,
                field.data_type.clone(),
                byte_order.clone(),
            )
        {
            results.push(ParsedField {
                name: field.name.clone(),
                value,
                data_type: field.data_type.clone(),
            });
        }
    }

    results
}

/// Encode field values to register values
///
/// This function converts field values to Modbus register values using
/// the DataTypeConverter. It's the inverse of parse_signal_group_fields.
///
/// # Arguments
///
/// * `fields_data` - Map of field name to value
/// * `fields` - Field mappings defining how to encode each field
/// * `register_count` - Total number of registers in the signal group
/// * `byte_order` - Byte order for multi-register types
///
/// # Returns
///
/// Vec<u16> with register values ready for Modbus write.
/// Returns None if a field name is not found in the mapping.
pub fn encode_fields_to_registers(
    fields_data: &serde_json::Map<String, serde_json::Value>,
    fields: &[FieldMapping],
    register_count: u16,
    byte_order: ByteOrder,
) -> Option<Vec<u16>> {
    // Initialize register array with zeros
    let mut registers = vec![0u16; register_count as usize];

    // Process each field in the input data
    for (field_name, field_value) in fields_data {
        // Find the field mapping
        let field = fields.iter().find(|f| &f.name == field_name)?;

        // Convert JSON value to f64
        let value = field_value.as_f64()?;

        // Convert value to bytes using DataTypeConverter
        let bytes =
            <crate::data_conversion::DefaultDataTypeConverter as DataTypeConverter>::to_bytes(
                value,
                field.data_type.clone(),
                byte_order.clone(),
            )?;

        // Calculate required registers
        let required_registers = field.data_type.required_registers();
        let end_offset = field.offset.saturating_add(required_registers);

        // Check bounds
        if end_offset > register_count {
            return None;
        }

        // Convert bytes to registers and place at offset
        let regs = bytes_to_registers(&bytes);
        for (i, reg) in regs.into_iter().enumerate() {
            registers[field.offset as usize + i] = reg;
        }
    }

    Some(registers)
}

/// Convert bytes to u16 register values
fn bytes_to_registers(bytes: &[u8]) -> Vec<u16> {
    let mut registers = Vec::new();
    let mut iter = bytes.iter();

    while let Some(&high) = iter.next() {
        if let Some(&low) = iter.next() {
            registers.push(((high as u16) << 8) | (low as u16));
        } else {
            registers.push(high as u16);
        }
    }

    registers
}

/// Extract bytes from register array at specified offset
///
/// # Arguments
///
/// * `registers` - Register array
/// * `offset` - Register offset (in registers, not bytes)
/// * `byte_count` - Number of bytes to extract
///
/// # Returns
///
/// Byte vector with extracted data
fn extract_bytes_from_registers(registers: &[u16], offset: usize, byte_count: usize) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(byte_count);
    let registers_needed = (byte_count + 1) / 2;

    for i in 0..registers_needed {
        if offset + i < registers.len() {
            let reg = registers[offset + i];
            // For single-byte values, extract low byte (LSB)
            // For multi-byte values, extract both bytes (MSB first for big-endian)
            if byte_count == 1 {
                // Single byte: use low byte (LSB) which contains the actual value
                bytes.push((reg & 0xFF) as u8);
            } else {
                // Multiple bytes: standard MSB-first extraction
                bytes.push((reg >> 8) as u8);
                bytes.push((reg & 0xFF) as u8);
            }
        }
    }

    bytes.truncate(byte_count);
    bytes
}

// ==================== Unit Tests ====================

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper to create a FieldMapping
    fn make_field(name: &str, data_type: DataType, offset: u16) -> FieldMapping {
        FieldMapping {
            name: name.to_string(),
            data_type,
            offset,
        }
    }

    #[test]
    fn parse_signal_group_with_u16_fields() {
        // Registers: [0x1234, 0x5678]
        // Expected: U16 at offset 0 = 0x1234 = 4660
        //           U16 at offset 1 = 0x5678 = 22136
        let registers = vec![0x1234, 0x5678];
        let fields = vec![
            make_field("field1", DataType::U16, 0),
            make_field("field2", DataType::U16, 1),
        ];

        let parsed = parse_signal_group_fields(&registers, &fields, ByteOrder::BigEndian);

        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].name, "field1");
        assert_eq!(parsed[0].value, 4660.0);
        assert_eq!(parsed[0].data_type, DataType::U16);
        assert_eq!(parsed[1].name, "field2");
        assert_eq!(parsed[1].value, 22136.0);
    }

    #[test]
    fn parse_signal_group_with_f32_field() {
        // F32 value 3.14159 in big-endian: 0x40490FD0
        // Split into two registers: 0x4049, 0x0FD0
        let registers = vec![0x4049, 0x0FD0];
        let fields = vec![make_field("pi", DataType::F32, 0)];

        let parsed = parse_signal_group_fields(&registers, &fields, ByteOrder::BigEndian);

        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].name, "pi");
        // Allow small floating point tolerance
        let expected = std::f32::consts::PI as f64;
        assert!((parsed[0].value - expected).abs() < 0.0001);
    }

    #[test]
    fn parse_signal_group_with_mixed_types() {
        // Registers: [0x1234, 0x5678, 0xABCD, 0xEF01]
        // U16 at offset 0: 0x1234 = 4660
        // F32 at offset 1: registers[1..2] = [0x5678, 0xABCD]
        // I16 at offset 3: 0xEF01 = -4351 (signed)
        let registers = vec![0x1234, 0x5678, 0xABCD, 0xEF01];
        let fields = vec![
            make_field("u16_val", DataType::U16, 0),
            make_field("f32_val", DataType::F32, 1),
            make_field("i16_val", DataType::I16, 3),
        ];

        let parsed = parse_signal_group_fields(&registers, &fields, ByteOrder::BigEndian);

        assert_eq!(parsed.len(), 3);
        assert_eq!(parsed[0].value, 4660.0);
        // F32 value from 0x5678ABCD bytes
        assert_eq!(parsed[2].value, -4351.0);
    }

    #[test]
    fn parse_field_with_offset() {
        // Registers: [0x0001, 0x0002, 0x0003, 0x0004]
        // U16 at offset 2: 0x0003 = 3
        let registers = vec![0x0001, 0x0002, 0x0003, 0x0004];
        let fields = vec![make_field("offset_field", DataType::U16, 2)];

        let parsed = parse_signal_group_fields(&registers, &fields, ByteOrder::BigEndian);

        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].value, 3.0);
    }

    #[test]
    fn parse_field_with_little_endian() {
        // Little endian: bytes are reversed
        // U32 from registers [0x1234, 0x5678]
        // Extract bytes: [0x12, 0x34, 0x56, 0x78]
        // After LittleEndian reversal: [0x78, 0x56, 0x34, 0x12]
        // Interpreted as BE: 0x78563412 = 2018915346
        let registers: Vec<u16> = vec![0x1234, 0x5678];
        let fields = vec![make_field("little_u32", DataType::U32, 0)];

        let parsed = parse_signal_group_fields(&registers, &fields, ByteOrder::LittleEndian);

        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].value, 2018915346.0);
    }

    #[test]
    fn parse_empty_signal_group() {
        let registers: Vec<u16> = vec![];
        let fields: Vec<FieldMapping> = vec![];

        let parsed = parse_signal_group_fields(&registers, &fields, ByteOrder::BigEndian);

        assert!(parsed.is_empty());
    }

    #[test]
    fn parse_signal_group_with_bool_true() {
        // Bool true: any non-zero value
        let registers = vec![0x0001];
        let fields = vec![make_field("flag", DataType::Bool, 0)];

        let parsed = parse_signal_group_fields(&registers, &fields, ByteOrder::BigEndian);

        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].value, 1.0);
    }

    #[test]
    fn parse_signal_group_with_bool_false() {
        // Bool false: zero value
        let registers = vec![0x0000];
        let fields = vec![make_field("flag", DataType::Bool, 0)];

        let parsed = parse_signal_group_fields(&registers, &fields, ByteOrder::BigEndian);

        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].value, 0.0);
    }

    #[test]
    fn parse_i16_negative_value() {
        // I16 -1 in big endian: 0xFFFF
        let registers = vec![0xFFFF];
        let fields = vec![make_field("negative", DataType::I16, 0)];

        let parsed = parse_signal_group_fields(&registers, &fields, ByteOrder::BigEndian);

        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].value, -1.0);
    }

    #[test]
    fn parse_u32_two_registers() {
        // U32 0x12345678 from two registers
        let registers = vec![0x1234, 0x5678];
        let fields = vec![make_field("u32_val", DataType::U32, 0)];

        let parsed = parse_signal_group_fields(&registers, &fields, ByteOrder::BigEndian);

        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].value, 305419896.0);
    }

    #[test]
    fn parse_i32_negative_value() {
        // I32 -1 in big endian: 0xFFFFFFFF
        let registers = vec![0xFFFF, 0xFFFF];
        let fields = vec![make_field("negative", DataType::I32, 0)];

        let parsed = parse_signal_group_fields(&registers, &fields, ByteOrder::BigEndian);

        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].value, -1.0);
    }

    #[test]
    fn parse_field_offset_out_of_bounds_skipped() {
        // Only 2 registers, but field at offset 2 needs 1 register
        let registers = vec![0x1234, 0x5678];
        let fields = vec![
            make_field("valid", DataType::U16, 0),
            make_field("out_of_bounds", DataType::U16, 2),
        ];

        let parsed = parse_signal_group_fields(&registers, &fields, ByteOrder::BigEndian);

        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].name, "valid");
    }

    #[test]
    fn parse_f32_offset_out_of_bounds_skipped() {
        // F32 needs 2 registers, only 1 register available at offset 1
        let registers = vec![0x1234, 0x5678];
        let fields = vec![
            make_field("valid_u16", DataType::U16, 0),
            make_field("out_of_bounds_f32", DataType::F32, 1),
        ];

        let parsed = parse_signal_group_fields(&registers, &fields, ByteOrder::BigEndian);

        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].name, "valid_u16");
    }

    #[test]
    fn extract_bytes_from_registers_single() {
        let registers = vec![0x1234];
        let bytes = extract_bytes_from_registers(&registers, 0, 2);

        assert_eq!(bytes, vec![0x12, 0x34]);
    }

    #[test]
    fn extract_bytes_from_registers_pair() {
        let registers = vec![0x1234, 0x5678];
        let bytes = extract_bytes_from_registers(&registers, 0, 4);

        assert_eq!(bytes, vec![0x12, 0x34, 0x56, 0x78]);
    }

    #[test]
    fn extract_bytes_from_registers_with_offset() {
        let registers = vec![0x0001, 0x0203, 0x0405];
        let bytes = extract_bytes_from_registers(&registers, 1, 4);

        assert_eq!(bytes, vec![0x02, 0x03, 0x04, 0x05]);
    }

    #[test]
    fn parse_with_little_endian_byte_swap() {
        // Test LittleEndianByteSwap byte order
        // Input registers: [0x1234, 0x5678] -> bytes: [0x12, 0x34, 0x56, 0x78]
        // LittleEndianByteSwap: swap bytes within each word -> [0x34, 0x12, 0x78, 0x56]
        let registers = vec![0x1234, 0x5678];
        let fields = vec![make_field("swapped", DataType::U32, 0)];

        let parsed =
            parse_signal_group_fields(&registers, &fields, ByteOrder::LittleEndianByteSwap);

        assert_eq!(parsed.len(), 1);
        // After swap and big-endian interpretation: 0x34127856 = 873625686
        assert_eq!(parsed[0].value, 873625686.0);
    }

    #[test]
    fn parse_with_mid_big_byte_order() {
        // Test MidBig byte order
        // Input registers: [0x1234, 0x5678] -> bytes: [0x12, 0x34, 0x56, 0x78]
        // MidBig: swap the two 16-bit words -> [0x56, 0x78, 0x12, 0x34]
        let registers = vec![0x1234, 0x5678];
        let fields = vec![make_field("midbig", DataType::U32, 0)];

        let parsed = parse_signal_group_fields(&registers, &fields, ByteOrder::MidBig);

        assert_eq!(parsed.len(), 1);
        // After swap and big-endian interpretation: 0x56781234 = 1450709556
        assert_eq!(parsed[0].value, 1450709556.0);
    }

    #[test]
    fn encode_fields_to_registers_u16() {
        let mut data = serde_json::Map::new();
        data.insert("value".to_string(), serde_json::json!(4660));

        let fields = vec![make_field("value", DataType::U16, 0)];

        let result = encode_fields_to_registers(&data, &fields, 2, ByteOrder::BigEndian);
        assert!(result.is_some());

        let registers = result.unwrap();
        assert_eq!(registers, vec![4660, 0]);
    }

    #[test]
    fn encode_fields_to_registers_f32() {
        let mut data = serde_json::Map::new();
        data.insert("pi".to_string(), serde_json::json!(3.14159));

        let fields = vec![make_field("pi", DataType::F32, 0)];

        let result = encode_fields_to_registers(&data, &fields, 2, ByteOrder::BigEndian);
        assert!(result.is_some());

        let registers = result.unwrap();
        assert_eq!(registers.len(), 2);
    }

    #[test]
    fn encode_fields_to_registers_unknown_field_returns_none() {
        let mut data = serde_json::Map::new();
        data.insert("unknown_field".to_string(), serde_json::json!(42));

        let fields = vec![make_field("value", DataType::U16, 0)];

        let result = encode_fields_to_registers(&data, &fields, 1, ByteOrder::BigEndian);
        assert!(result.is_none());
    }

    #[test]
    fn encode_fields_to_registers_offset_placement() {
        let mut data = serde_json::Map::new();
        data.insert("value".to_string(), serde_json::json!(123));

        let fields = vec![make_field("value", DataType::U16, 3)];

        let result = encode_fields_to_registers(&data, &fields, 4, ByteOrder::BigEndian);
        assert!(result.is_some());

        let registers = result.unwrap();
        assert_eq!(registers, vec![0, 0, 0, 123]);
    }
}
