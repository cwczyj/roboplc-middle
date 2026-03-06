//! Modbus operation types and utilities
//!
//! This module provides the RegisterType enum and address parsing utilities
//! for Modbus operations.

use roboplc::io::modbus::prelude::ModbusRegisterKind;

/// Represents the four main Modbus register types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegisterType {
    /// Coil (0x range) - boolean, read/write
    Coil,
    /// Discrete Input (1x range) - boolean, read-only
    Discrete,
    /// Input Register (3x range) - 16-bit, read-only
    Input,
    /// Holding Register (4x range) - 16-bit, read/write
    Holding,
}

impl RegisterType {
    /// Returns the prefix character for this register type
    pub fn prefix(&self) -> char {
        match self {
            RegisterType::Coil => 'c',
            RegisterType::Discrete => 'd',
            RegisterType::Input => 'i',
            RegisterType::Holding => 'h',
        }
    }

    /// Returns true if this register type is read-only (Discrete or Input)
    pub fn is_read_only(&self) -> bool {
        matches!(self, RegisterType::Discrete | RegisterType::Input)
    }

    /// Returns true if this register type is writable (Coil or Holding)
    pub fn is_writable(&self) -> bool {
        matches!(self, RegisterType::Coil | RegisterType::Holding)
    }

    /// Convert to ModbusRegisterKind enum
    pub fn to_modbus_register_kind(&self) -> ModbusRegisterKind {
        match self {
            RegisterType::Coil => ModbusRegisterKind::Coil,
            RegisterType::Discrete => ModbusRegisterKind::Discrete,
            RegisterType::Input => ModbusRegisterKind::Input,
            RegisterType::Holding => ModbusRegisterKind::Holding,
        }
    }
}

impl std::fmt::Display for RegisterType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RegisterType::Coil => write!(f, "Coil"),
            RegisterType::Discrete => write!(f, "Discrete"),
            RegisterType::Input => write!(f, "Input"),
            RegisterType::Holding => write!(f, "Holding"),
        }
    }
}

/// Parse a register address string into register type and address number
///
/// # Address Format
/// - `c0` - Coil at address 0
/// - `d5` - Discrete Input at address 5
/// - `i50` - Input Register at address 50
/// - `h100` - Holding Register at address 100
/// - `100` - No prefix defaults to Holding Register
///
/// # Arguments
/// * `addr_str` - Address string (e.g., "h100", "c0", "50")
///
/// # Returns
/// * `Some((RegisterType, u16))` - Register type and address number
/// * `None` - If parsing fails (empty string, invalid number)
///
/// # Examples
/// ```
/// use roboplc_middleware::workers::modbus::{RegisterType, parse_register_address};
/// let (reg_type, addr) = parse_register_address("h100").unwrap();
/// assert_eq!(reg_type, RegisterType::Holding);
/// assert_eq!(addr, 100);
/// ```
pub fn parse_register_address(addr_str: &str) -> Option<(RegisterType, u16)> {
    let addr_str = addr_str.trim();
    if addr_str.is_empty() {
        return None;
    }

    // Check for prefix (h, i, c, d) - case insensitive
    let (reg_type, num_part) = if addr_str.starts_with('h') || addr_str.starts_with('H') {
        (RegisterType::Holding, &addr_str[1..])
    } else if addr_str.starts_with('i') || addr_str.starts_with('I') {
        (RegisterType::Input, &addr_str[1..])
    } else if addr_str.starts_with('c') || addr_str.starts_with('C') {
        (RegisterType::Coil, &addr_str[1..])
    } else if addr_str.starts_with('d') || addr_str.starts_with('D') {
        (RegisterType::Discrete, &addr_str[1..])
    } else {
        // No prefix, assume holding register
        (RegisterType::Holding, addr_str)
    };

    // Parse the numeric part
    let address = num_part.parse::<u16>().ok()?;
    Some((reg_type, address))
}

/// Convert RoboPLC's ModbusRegisterKind to our RegisterType
pub fn register_type_from_kind(kind: ModbusRegisterKind) -> RegisterType {
    match kind {
        ModbusRegisterKind::Coil => RegisterType::Coil,
        ModbusRegisterKind::Discrete => RegisterType::Discrete,
        ModbusRegisterKind::Input => RegisterType::Input,
        ModbusRegisterKind::Holding => RegisterType::Holding,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_coil_address_with_c_prefix() {
        let result = parse_register_address("c0");
        assert_eq!(result, Some((RegisterType::Coil, 0)));

        let result = parse_register_address("C100");
        assert_eq!(result, Some((RegisterType::Coil, 100)));
    }

    #[test]
    fn parse_discrete_address_with_d_prefix() {
        let result = parse_register_address("d5");
        assert_eq!(result, Some((RegisterType::Discrete, 5)));

        let result = parse_register_address("D50");
        assert_eq!(result, Some((RegisterType::Discrete, 50)));
    }

    #[test]
    fn parse_input_address_with_i_prefix() {
        let result = parse_register_address("i50");
        assert_eq!(result, Some((RegisterType::Input, 50)));

        let result = parse_register_address("I200");
        assert_eq!(result, Some((RegisterType::Input, 200)));
    }

    #[test]
    fn parse_holding_address_with_h_prefix() {
        let result = parse_register_address("h100");
        assert_eq!(result, Some((RegisterType::Holding, 100)));

        let result = parse_register_address("H4000");
        assert_eq!(result, Some((RegisterType::Holding, 4000)));
    }

    #[test]
    fn parse_address_without_prefix_defaults_to_holding() {
        let result = parse_register_address("100");
        assert_eq!(result, Some((RegisterType::Holding, 100)));

        let result = parse_register_address("0");
        assert_eq!(result, Some((RegisterType::Holding, 0)));

        let result = parse_register_address("65535");
        assert_eq!(result, Some((RegisterType::Holding, 65535)));
    }

    #[test]
    fn parse_empty_address_returns_none() {
        assert_eq!(parse_register_address(""), None);
        assert_eq!(parse_register_address("   "), None);
    }

    #[test]
    fn parse_invalid_number_returns_none() {
        assert_eq!(parse_register_address("habc"), None);
        assert_eq!(parse_register_address("xyz"), None);
        assert_eq!(parse_register_address("h-1"), None);
        assert_eq!(parse_register_address("h999999"), None);
    }

    #[test]
    fn register_type_display() {
        assert_eq!(format!("{}", RegisterType::Coil), "Coil");
        assert_eq!(format!("{}", RegisterType::Discrete), "Discrete");
        assert_eq!(format!("{}", RegisterType::Input), "Input");
        assert_eq!(format!("{}", RegisterType::Holding), "Holding");
    }

    #[test]
    fn register_type_prefix() {
        assert_eq!(RegisterType::Coil.prefix(), 'c');
        assert_eq!(RegisterType::Discrete.prefix(), 'd');
        assert_eq!(RegisterType::Input.prefix(), 'i');
        assert_eq!(RegisterType::Holding.prefix(), 'h');
    }

    #[test]
    fn register_type_is_read_only() {
        // Discrete and Input are read-only
        assert!(!RegisterType::Coil.is_read_only());
        assert!(RegisterType::Discrete.is_read_only());
        assert!(RegisterType::Input.is_read_only());
        assert!(!RegisterType::Holding.is_read_only());
    }

    #[test]
    fn register_type_is_writable() {
        // Coil and Holding are writable
        assert!(RegisterType::Coil.is_writable());
        assert!(!RegisterType::Discrete.is_writable());
        assert!(!RegisterType::Input.is_writable());
        assert!(RegisterType::Holding.is_writable());
    }

    #[test]
    fn register_type_to_modbus_register_kind() {
        //! Modbus operation types and utilities
        //!
        //! This module provides the RegisterType enum and address parsing utilities
        //! for Modbus operations.

        use roboplc::io::modbus::prelude::ModbusRegisterKind;

        assert_eq!(
            RegisterType::Coil.to_modbus_register_kind(),
            ModbusRegisterKind::Coil
        );
        assert_eq!(
            RegisterType::Discrete.to_modbus_register_kind(),
            ModbusRegisterKind::Discrete
        );
        assert_eq!(
            RegisterType::Input.to_modbus_register_kind(),
            ModbusRegisterKind::Input
        );
        assert_eq!(
            RegisterType::Holding.to_modbus_register_kind(),
            ModbusRegisterKind::Holding
        );
    }

    #[test]
    fn register_type_from_kind_converts_correctly() {
        use roboplc::io::modbus::prelude::ModbusRegisterKind;

        assert_eq!(
            register_type_from_kind(ModbusRegisterKind::Coil),
            RegisterType::Coil
        );
        assert_eq!(
            register_type_from_kind(ModbusRegisterKind::Discrete),
            RegisterType::Discrete
        );
        assert_eq!(
            register_type_from_kind(ModbusRegisterKind::Input),
            RegisterType::Input
        );
        assert_eq!(
            register_type_from_kind(ModbusRegisterKind::Holding),
            RegisterType::Holding
        );
    }
}
