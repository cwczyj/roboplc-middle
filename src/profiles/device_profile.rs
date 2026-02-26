use crate::config::{AddressingMode, ByteOrder, DataType};
use binrw::{BinRead, BinWrite};
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

// Build a DeviceProfile from a device config
impl DeviceProfile {
    pub fn from_device(device: &crate::config::Device) -> Self {
        Self {
            device_id: device.id.clone(),
            addressing_mode: device.addressing_mode.clone(),
            byte_order: device.byte_order.clone(),
            register_mappings: device
                .register_mappings
                .iter()
                .map(|m| RegisterProfile {
                    signal_name: m.signal_name.clone(),
                    address: parse_address_number(&m.address).unwrap_or(0),
                    register_type: parse_register_type(&m.address)
                        .unwrap_or(RegisterType::HoldingRegister),
                    data_type: m.data_type.clone(),
                    scale_factor: None,
                    offset: None,
                })
                .collect(),
        }
    }
}

fn parse_address_number(addr: &str) -> Option<u16> {
    AddressMapping::parse(addr).map(|m| m.address)
}

fn parse_register_type(addr: &str) -> Option<RegisterType> {
    AddressMapping::parse(addr).map(|m| m.register_type)
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

#[derive(BinRead, BinWrite, Clone, Debug)]
pub enum RegisterValue {
    U16(u16),
    I16(i16),
}

#[derive(BinRead, BinWrite, Clone, Debug)]
pub struct RegisterPair {
    pub high: u16,
    pub low: u16,
}

impl RegisterPair {
    pub fn to_u32(&self) -> u32 {
        ((self.high as u32) << 16) | (self.low as u32)
    }
    
    pub fn to_i32(&self) -> i32 {
        self.to_u32() as i32
    }
    
    pub fn to_f32(&self) -> f32 {
        f32::from_bits(self.to_u32())
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
    use crate::config::{
        AccessMode, AddressingMode as ConfigAddressingMode, ByteOrder as ConfigByteOrder,
        DataType as ConfigDataType, Device as ConfigDevice, DeviceType as ConfigDeviceType,
        RegisterMapping,
    };

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

    #[test]
    fn test_from_device_basic() {
        // Build a minimal RegisterMapping and Device config
        let reg = crate::config::RegisterMapping {
            signal_name: "sig".to_string(),
            address: "h10".to_string(),
            data_type: ConfigDataType::U16,
            access: AccessMode::Rw,
            description: String::new(),
        };

        let device = crate::config::Device {
            id: "dev1".to_string(),
            device_type: ConfigDeviceType::Plc,
            address: "127.0.0.1".to_string(),
            port: 0,
            unit_id: 0,
            addressing_mode: ConfigAddressingMode::ZeroBased,
            byte_order: ConfigByteOrder::BigEndian,
            tcp_nodelay: true,
            max_concurrent_ops: 3,
            heartbeat_interval_sec: 30,
            register_mappings: vec![reg],
        };

        let profile = DeviceProfile::from_device(&device);
        assert_eq!(profile.device_id, "dev1");
        assert_eq!(profile.register_mappings.len(), 1);
        let rp = &profile.register_mappings[0];
        assert_eq!(rp.signal_name, "sig");
        assert_eq!(rp.address, 10);
        assert_eq!(rp.register_type, RegisterType::HoldingRegister);
        assert_eq!(rp.data_type, ConfigDataType::U16);
        assert!(rp.scale_factor.is_none());
        assert!(rp.offset.is_none());
    }
    #[test]
    fn test_register_pair_u32() {
        let pair = RegisterPair { high: 0x1234, low: 0x5678 };
        assert_eq!(pair.to_u32(), 0x12345678);
    }
    
    #[test]
    fn test_register_pair_f32() {
        let pair = RegisterPair { high: 0x4049, low: 0x0FDB };
        assert!((pair.to_f32() - 3.14159).abs() < 0.001);
    }
    
    #[test]
    fn test_register_pair_i32() {
        let pair = RegisterPair { high: 0xFFFF, low: 0xFFFF };
        assert_eq!(pair.to_i32(), -1);
    }
    
    #[test]
    fn test_register_pair_zero() {
        let pair = RegisterPair { high: 0, low: 0 };
        assert_eq!(pair.to_u32(), 0);
        assert_eq!(pair.to_i32(), 0);
        assert_eq!(pair.to_f32(), 0.0);
    }

}
