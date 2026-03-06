//! # Data Conversion Module
//!
//! Provides data type conversion utilities for Modbus register handling.
//! Handles conversion between byte arrays and numeric types with support
//! for various byte orders (BigEndian, LittleEndian, etc.).
//!
//! ## Main Types
//!
//! - [`DataTypeConverter`]: Trait for converting between bytes and numeric types
//! - [`DefaultDataTypeConverter`]: Default implementation of the converter trait
//! - [`RegisterPair`]: Helper for combining two 16-bit registers into 32-bit values
//!
//! ## Supported Data Types
//!
//! - U16: Unsigned 16-bit integer (1 register)
//! - U32: Unsigned 32-bit integer (2 registers)
//! - I16: Signed 16-bit integer (1 register)
//! - I32: Signed 32-bit integer (2 registers)
//! - F32: 32-bit floating point (2 registers)
//! - Bool: Boolean value (1 register, non-zero = true)
//!
//! ## Byte Order Support
//!
//! - BigEndian: MSB first (default Modbus)
//! - LittleEndian: LSB first
//! - LittleEndianByteSwap: Swap bytes within each 16-bit word
//! - MidBig: Swap 16-bit words

// ========== Module Imports ==========
use crate::config::{ByteOrder, DataType};

// ========== Public Types ==========

/// Trait for converting between byte arrays and numeric types.
///
/// This trait provides conversion functionality for Modbus register data,
/// supporting various data types and byte orders used in industrial protocols.
pub trait DataTypeConverter {
    /// Convert bytes to a floating-point value.
    ///
    /// # Arguments
    ///
    /// * `data` - Raw byte slice from Modbus register
    /// * `data_type` - Target data type for conversion
    /// * `byte_order` - Byte order for multi-byte types
    ///
    /// # Returns
    ///
    /// Some(f64) on success, None if conversion fails (e.g., wrong length)
    fn from_bytes(data: &[u8], data_type: DataType, byte_order: ByteOrder) -> Option<f64>;

    /// Convert a floating-point value to bytes.
    ///
    /// # Arguments
    ///
    /// * `value` - Numeric value to convert
    /// * `data_type` - Target data type
    /// * `byte_order` - Byte order for output
    ///
    /// # Returns
    ///
    /// Some(Vec<u8>) on success, None if conversion fails
    fn to_bytes(value: f64, data_type: DataType, byte_order: ByteOrder) -> Option<Vec<u8>>;
}

/// Default implementation of DataTypeConverter.
///
/// Provides standard conversion for all supported data types.
pub struct DefaultDataTypeConverter;

impl DataTypeConverter for DefaultDataTypeConverter {
    fn from_bytes(data: &[u8], data_type: DataType, byte_order: ByteOrder) -> Option<f64> {
        let expected_len = bytes_len(&data_type)?;
        if data.len() != expected_len {
            return None;
        }

        let ordered = convert_byte_order(data, byte_order);
        match data_type {
            DataType::U16 => Some(u16::from_be_bytes([ordered[0], ordered[1]]) as f64),
            DataType::U32 => {
                Some(u32::from_be_bytes([ordered[0], ordered[1], ordered[2], ordered[3]]) as f64)
            }
            DataType::I16 => Some(i16::from_be_bytes([ordered[0], ordered[1]]) as f64),
            DataType::I32 => {
                Some(i32::from_be_bytes([ordered[0], ordered[1], ordered[2], ordered[3]]) as f64)
            }
            DataType::F32 => {
                Some(f32::from_be_bytes([ordered[0], ordered[1], ordered[2], ordered[3]]) as f64)
            }
            DataType::Bool => Some((ordered[0] != 0) as u8 as f64),
        }
    }

    fn to_bytes(value: f64, data_type: DataType, byte_order: ByteOrder) -> Option<Vec<u8>> {
        let canonical = match data_type {
            DataType::U16 => Some((value as u16).to_be_bytes().to_vec()),
            DataType::U32 => Some((value as u32).to_be_bytes().to_vec()),
            DataType::I16 => Some((value as i16).to_be_bytes().to_vec()),
            DataType::I32 => Some((value as i32).to_be_bytes().to_vec()),
            DataType::F32 => Some((value as f32).to_be_bytes().to_vec()),
            DataType::Bool => Some(vec![(value != 0.0) as u8]),
        }?;

        Some(convert_byte_order(&canonical, byte_order))
    }
}

/// Helper struct for combining two 16-bit registers into 32-bit values.
///
/// Many Modbus devices store 32-bit values across two consecutive registers.
/// This struct provides convenient methods for extracting those values.
#[derive(Debug, Clone, Copy, Default)]
pub struct RegisterPair {
    /// High 16 bits (first register in Modbus convention)
    pub high: u16,
    /// Low 16 bits (second register in Modbus convention)
    pub low: u16,
}

impl RegisterPair {
    /// Create a new RegisterPair from high and low values.
    pub fn new(high: u16, low: u16) -> Self {
        Self { high, low }
    }

    /// Combine high and low registers into a u32 value.
    ///
    /// Result = (high as u32) << 16 | (low as u32)
    pub fn to_u32(&self) -> u32 {
        ((self.high as u32) << 16) | (self.low as u32)
    }

    /// Combine high and low registers into a signed i32 value.
    pub fn to_i32(&self) -> i32 {
        self.to_u32() as i32
    }

    /// Combine high and low registers into a f32 value.
    ///
    /// Interprets the 32-bit pattern as an IEEE 754 float.
    pub fn to_f32(&self) -> f32 {
        f32::from_bits(self.to_u32())
    }
}

// ========== Private Helper Functions ==========

/// Get the expected byte length for a given data type.
fn bytes_len(data_type: &DataType) -> Option<usize> {
    match data_type {
        DataType::U16 | DataType::I16 => Some(2),
        DataType::U32 | DataType::I32 | DataType::F32 => Some(4),
        DataType::Bool => Some(1),
    }
}

/// Convert byte order based on the specified ByteOrder enum.
///
/// This function handles all standard byte orders used in industrial protocols:
/// - BigEndian: No change (MSB first)
/// - LittleEndian: Reverse all bytes
/// - LittleEndianByteSwap: Swap bytes within each 16-bit word
/// - MidBig: Swap the two 16-bit words
pub fn convert_byte_order(data: &[u8], byte_order: ByteOrder) -> Vec<u8> {
    match byte_order {
        ByteOrder::BigEndian => data.to_vec(),
        ByteOrder::LittleEndian => data.iter().rev().copied().collect(),
        ByteOrder::LittleEndianByteSwap => data
            .chunks(2)
            .flat_map(|chunk| chunk.iter().rev().copied())
            .collect(),
        ByteOrder::MidBig => data
            .chunks(2)
            .collect::<Vec<_>>()
            .chunks(2)
            .flat_map(|pair| pair.iter().rev().flat_map(|c| c.iter().copied()))
            .collect(),
    }
}

// ========== Unit Tests ==========

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{ByteOrder, DataType};
    use crate::data_conversion::{DataTypeConverter, DefaultDataTypeConverter};

    // Test U16 to F64 conversion
    #[test]
    fn convert_u16_to_f64() {
        // 0x1234 = 4660
        let bytes = [0x12, 0x34];
        let result = <DefaultDataTypeConverter as DataTypeConverter>::from_bytes(
            &bytes,
            DataType::U16,
            ByteOrder::BigEndian,
        );
        assert_eq!(result, Some(4660.0));
    }

    // Test I32 with negative value
    #[test]
    fn convert_i32_with_negative_value() {
        // -1 in big endian is 0xFFFFFFFF
        let bytes = [0xFF, 0xFF, 0xFF, 0xFF];
        let result = <DefaultDataTypeConverter as DataTypeConverter>::from_bytes(
            &bytes,
            DataType::I32,
            ByteOrder::BigEndian,
        );
        assert_eq!(result, Some(-1.0));
    }

    // Test F32 preserves precision
    #[test]
    fn convert_f32_preserves_precision() {
        let test_value = 3.14159_f32;
        let bytes = test_value.to_be_bytes();
        let result = <DefaultDataTypeConverter as DataTypeConverter>::from_bytes(
            &bytes,
            DataType::F32,
            ByteOrder::BigEndian,
        );
        let converted_back = result.unwrap() as f32;
        // Allow small floating point tolerance
        assert!((converted_back - test_value).abs() < 0.0001);
    }

    // Test BigEndian byte order
    #[test]
    fn convert_with_byte_order_big_endian() {
        let bytes = [0x12, 0x34, 0x56, 0x78];
        let result = <DefaultDataTypeConverter as DataTypeConverter>::from_bytes(
            &bytes,
            DataType::U32,
            ByteOrder::BigEndian,
        );
        // 0x12345678 = 305419896
        assert_eq!(result, Some(305419896.0));
    }

    // Test LittleEndian byte order
    #[test]
    fn convert_with_byte_order_little_endian() {
        // Same bytes, but interpreted as little endian
        // 0x78563412 = 2018915346
        let bytes = [0x12, 0x34, 0x56, 0x78];
        let result = <DefaultDataTypeConverter as DataTypeConverter>::from_bytes(
            &bytes,
            DataType::U32,
            ByteOrder::LittleEndian,
        );
        assert_eq!(result, Some(2018915346.0));
    }

    // Test RegisterPair to_u32
    #[test]
    fn register_pair_to_u32() {
        let pair = RegisterPair::new(0x1234, 0x5678);
        let result = pair.to_u32();
        assert_eq!(result, 0x12345678);
    }

    // Test RegisterPair to_f32
    #[test]
    fn register_pair_to_f32() {
        let test_value = 12.5_f32;
        let bits = test_value.to_be_bytes();
        let pair = RegisterPair::new(
            u16::from_be_bytes([bits[0], bits[1]]),
            u16::from_be_bytes([bits[2], bits[3]]),
        );
        let result = pair.to_f32();
        assert_eq!(result, test_value);
    }

    // Additional tests for completeness

    #[test]
    fn convert_bool_true() {
        let bytes = [0x01];
        let result = <DefaultDataTypeConverter as DataTypeConverter>::from_bytes(
            &bytes,
            DataType::Bool,
            ByteOrder::BigEndian,
        );
        assert_eq!(result, Some(1.0));
    }

    #[test]
    fn convert_bool_false() {
        let bytes = [0x00];
        let result = <DefaultDataTypeConverter as DataTypeConverter>::from_bytes(
            &bytes,
            DataType::Bool,
            ByteOrder::BigEndian,
        );
        assert_eq!(result, Some(0.0));
    }

    #[test]
    fn convert_i16_positive() {
        // 0x0123 = 291
        let bytes = [0x01, 0x23];
        let result = <DefaultDataTypeConverter as DataTypeConverter>::from_bytes(
            &bytes,
            DataType::I16,
            ByteOrder::BigEndian,
        );
        assert_eq!(result, Some(291.0));
    }

    #[test]
    fn convert_i16_negative() {
        // -1 in big endian is 0xFFFF
        let bytes = [0xFF, 0xFF];
        let result = <DefaultDataTypeConverter as DataTypeConverter>::from_bytes(
            &bytes,
            DataType::I16,
            ByteOrder::BigEndian,
        );
        assert_eq!(result, Some(-1.0));
    }

    #[test]
    fn to_bytes_u16_roundtrip() {
        let value = 12345.0;
        let bytes = <DefaultDataTypeConverter as DataTypeConverter>::to_bytes(
            value,
            DataType::U16,
            ByteOrder::BigEndian,
        )
        .unwrap();
        let result = <DefaultDataTypeConverter as DataTypeConverter>::from_bytes(
            &bytes,
            DataType::U16,
            ByteOrder::BigEndian,
        )
        .unwrap();
        assert_eq!(result, value);
    }

    #[test]
    fn to_bytes_f32_roundtrip() {
        let value = 98.765_f64;
        let bytes = <DefaultDataTypeConverter as DataTypeConverter>::to_bytes(
            value,
            DataType::F32,
            ByteOrder::BigEndian,
        )
        .unwrap();
        let result = <DefaultDataTypeConverter as DataTypeConverter>::from_bytes(
            &bytes,
            DataType::F32,
            ByteOrder::BigEndian,
        )
        .unwrap();
        let result_f32 = result as f32;
        let value_f32 = value as f32;
        assert!((result_f32 - value_f32).abs() < 0.001);
    }

    #[test]
    fn bytes_len_correct() {
        assert_eq!(bytes_len(&DataType::U16), Some(2));
        assert_eq!(bytes_len(&DataType::I16), Some(2));
        assert_eq!(bytes_len(&DataType::U32), Some(4));
        assert_eq!(bytes_len(&DataType::I32), Some(4));
        assert_eq!(bytes_len(&DataType::F32), Some(4));
        assert_eq!(bytes_len(&DataType::Bool), Some(1));
    }

    #[test]
    fn convert_byte_order_little_endian_byte_swap() {
        // Input: [0x12, 0x34, 0x56, 0x78]
        // LittleEndianByteSwap: swap bytes within each word
        // Result: [0x34, 0x12, 0x78, 0x56]
        let input = [0x12, 0x34, 0x56, 0x78];
        let result = convert_byte_order(&input, ByteOrder::LittleEndianByteSwap);
        assert_eq!(result, [0x34, 0x12, 0x78, 0x56]);
    }

    #[test]
    fn convert_byte_order_mid_big() {
        // Input: [0x12, 0x34, 0x56, 0x78]
        // MidBig: swap 16-bit words
        // Result: [0x56, 0x78, 0x12, 0x34]
        let input = [0x12, 0x34, 0x56, 0x78];
        let result = convert_byte_order(&input, ByteOrder::MidBig);
        assert_eq!(result, [0x56, 0x78, 0x12, 0x34]);
    }

    #[test]
    fn wrong_length_returns_none() {
        // U16 expects 2 bytes, give 4
        let bytes = [0x12, 0x34, 0x56, 0x78];
        let result = <DefaultDataTypeConverter as DataTypeConverter>::from_bytes(
            &bytes,
            DataType::U16,
            ByteOrder::BigEndian,
        );
        assert_eq!(result, None);
    }
}
