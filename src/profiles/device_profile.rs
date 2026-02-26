use crate::config::{AddressingMode, ByteOrder, DataType};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceProfile {
    pub device_id: String,
    pub addressing_mode: AddressingMode,
    pub byte_order: ByteOrder,
    pub register_mappings: Vec<RegisterProfile>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum RegisterType {
    Coil,
    DiscreteInput,
    InputRegister,
    HoldingRegister,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct AddressMapping {
    pub register_type: RegisterType,
    pub address: u16,
}

impl AddressMapping {
    pub fn parse(addr: &str) -> Option<Self> {
        let addr = addr.trim();
        if addr.len() < 2 {
            return None;
        }

        let (prefix, num_str) = addr.split_at(1);
        let offset: u16 = num_str.parse().ok()?;

        let register_type = match prefix.to_ascii_lowercase().as_str() {
            "c" => RegisterType::Coil,
            "d" => RegisterType::DiscreteInput,
            "i" => RegisterType::InputRegister,
            "h" => RegisterType::HoldingRegister,
            _ => return None,
        };

        Some(Self {
            register_type,
            address: offset,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisterProfile {
    pub signal_name: String,
    pub address: u16,
    pub register_type: RegisterType,
    pub data_type: DataType,
    pub scale_factor: Option<f32>,
    pub offset: Option<f32>,
}

impl RegisterProfile {
    pub fn parse_address(addr: &str) -> Option<(RegisterType, u16)> {
        let parsed = AddressMapping::parse(addr)?;
        Some((parsed.register_type, parsed.address))
    }
}

pub trait DataTypeConverter {
    fn from_bytes(data: &[u8], data_type: DataType, byte_order: ByteOrder) -> Option<f64>;
    fn to_bytes(value: f64, data_type: DataType, byte_order: ByteOrder) -> Option<Vec<u8>>;
}

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

fn bytes_len(data_type: &DataType) -> Option<usize> {
    match data_type {
        DataType::U16 | DataType::I16 => Some(2),
        DataType::U32 | DataType::I32 | DataType::F32 => Some(4),
        DataType::Bool => Some(1),
    }
}

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_address_mapping() {
        assert_eq!(
            AddressMapping::parse("h100"),
            Some(AddressMapping {
                register_type: RegisterType::HoldingRegister,
                address: 100
            })
        );
        assert_eq!(
            AddressMapping::parse(" c0 "),
            Some(AddressMapping {
                register_type: RegisterType::Coil,
                address: 0
            })
        );
        assert_eq!(
            AddressMapping::parse("i50"),
            Some(AddressMapping {
                register_type: RegisterType::InputRegister,
                address: 50
            })
        );
        assert_eq!(
            AddressMapping::parse("d5"),
            Some(AddressMapping {
                register_type: RegisterType::DiscreteInput,
                address: 5
            })
        );
        assert_eq!(AddressMapping::parse("x10"), None);
        assert_eq!(AddressMapping::parse("h"), None);
    }

    #[test]
    fn convert_endianness_variants() {
        let input = [0x11, 0x22, 0x33, 0x44];
        assert_eq!(
            convert_byte_order(&input, ByteOrder::BigEndian),
            vec![0x11, 0x22, 0x33, 0x44]
        );
        assert_eq!(
            convert_byte_order(&input, ByteOrder::LittleEndian),
            vec![0x44, 0x33, 0x22, 0x11]
        );
        assert_eq!(
            convert_byte_order(&input, ByteOrder::LittleEndianByteSwap),
            vec![0x22, 0x11, 0x44, 0x33]
        );
        assert_eq!(
            convert_byte_order(&input, ByteOrder::MidBig),
            vec![0x33, 0x44, 0x11, 0x22]
        );
    }
}
