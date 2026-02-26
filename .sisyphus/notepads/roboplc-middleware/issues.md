# Issues

## 2026-02-26 System Linker Issue

### Issue
- Error: `linker 'cc' not found`
- Dependencies resolve correctly but compilation fails due to missing C compiler

### Impact
- Cannot compile Rust code on this system
- Need to install build-essential or equivalent

### Workaround
- Document that Cargo.toml is correct
- Dependencies successfully downloaded and locked
- System needs `apt install build-essential` or equivalent

### Status
- Blocked: Requires system-level package installation
- Cargo.toml is verified correct