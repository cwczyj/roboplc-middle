// ============================================================================
// profiles 模块 - 设备配置文件管理模块
// ============================================================================
// pub (public的缩写): 可见性修饰符,表示这个模块对外公开,其他代码可以访问
// mod (module的缩写): 声明一个子模块,用于代码组织和封装
// device_profile: 子模块的名称,对应同名的 device_profile.rs 文件
//
// Rust 模块系统说明:
// - 每个 .rs 文件默认是一个模块
// - mod.rs 文件是该目录模块的入口点
// - 使用 pub mod 可以让外部代码访问这个子模块
// - 例如: crate::profiles::device_profile::DeviceProfile
//
// 文件结构:
// src/profiles/
//   ├── mod.rs          (当前文件 - 模块入口)
//   └── device_profile.rs (子模块 - 设备配置实现)
//
// 使用方法:
// use crate::profiles::device_profile::{DeviceProfile, RegisterType};
// ============================================================================
pub mod device_profile;
