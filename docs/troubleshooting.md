# 故障排除指南

本文档列出了 roboplc-middleware 常见问题及其解决方案。

## 连接问题

### 无法连接到 Modbus 设备

**症状:**
- 日志显示 "Connection refused" 或 "Timeout"
- 设备状态一直为 "Disconnected"

**可能原因:**
1. 设备 IP 地址或端口错误
2. 网络不通
3. 设备未启动或 Modbus 服务未运行
4. 防火墙阻止连接

**解决方案:**
```bash
# 1. 检查网络连通性
ping 192.168.1.100

# 2. 检查端口是否开放
nc -zv 192.168.1.100 502

# 3. 检查防火墙规则
sudo iptables -L -n | grep 502

# 4. 使用 modpoll 工具测试
modpoll -m tcp -t 3 -r 100 -c 10 192.168.1.100
```

### 连接建立后立即断开

**症状:**
- 连接成功但立即断开
- 日志显示频繁的 "Connected" 和 "Disconnected"

**可能原因:**
1. `tcp_nodelay` 设置与设备不兼容
2. 设备不支持多连接
3. 连接超时设置过短
4. 心跳间隔设置过短

**解决方案:**
```toml
# 尝试禁用 tcp_nodelay
[[devices]]
tcp_nodelay = false

# 增加心跳间隔
heartbeat_interval_sec = 60
```

### 连接超时

**症状:**
- 日志显示 "Connection timed out"
- 设备状态在 "Connecting" 和 "Disconnected" 之间切换

**可能原因:**
1. 网络延迟过高
2. 设备响应慢
3. 网络不稳定

**解决方案:**
- 检查网络质量
- 增加超时设置（在代码中调整 TimeoutHandler）
- 使用更稳定的网络连接

## 数据读取问题

### 读取的值不正确

**症状:**
- 数值与预期不符
- 数值出现异常大的值或负值

**可能原因:**
1. 字节序设置错误
2. 地址模式设置错误
3. 数据类型不匹配
4. 字段偏移量错误

**解决方案:**

**检查字节序:**
```toml
# 尝试不同的字节序
byte_order = "little_endian"  # 或 "big_endian" / "little_endian_byte_swap" / "mid_big"
```

**检查地址模式:**
```toml
# 如果读取的地址偏移 1，尝试切换地址模式
addressing_mode = "one_based"  # 或 "zero_based"
```

**验证字段偏移量:**
```toml
# 确保字段偏移量 + 数据类型所需寄存器数 ≤ register_count
[[devices.signal_groups.fields]]
name = "temperature"
data_type = "F32"  # 占用 2 个寄存器
offset = 0         # 起始位置

[[devices.signal_groups.fields]]
name = "humidity"
data_type = "U16"  # 占用 1 个寄存器
offset = 2         # 必须 ≥ 上一个字段的 offset + 所需寄存器数
```

### 读取寄存器返回错误

**症状:**
- Modbus 异常码错误
- 部分地址读取失败

**可能原因:**
1. 地址超出设备范围
2. 设备不支持该功能码
3. 单元 ID 错误
4. 信号组配置错误

**解决方案:**
```bash
# 使用 modpoll 工具测试
modpoll -m tcp -t 3 -r 100 -c 10 192.168.1.100

# 检查设备手册确认支持的地址范围
# 验证 unit_id 与设备配置一致
```

### 字段值解析错误

**症状:**
- F32 值显示为极大或极小的数值
- Bool 值不正确
- U32/I32 值异常

**可能原因:**
1. 字节序与设备不匹配
2. 字段偏移量跨越了寄存器边界
3. 数据类型配置错误

**解决方案:**
- 参考设备手册确认字节序
- 使用 `tests/mock_modbus.rs` 测试数据解析
- 验证字段偏移量不重叠

## 性能问题

### 响应延迟过高

**症状:**
- 响应时间超过预期
- 偶发超时

**可能原因:**
1. 并发操作过多
2. 网络延迟
3. 设备处理能力不足
4. 心跳检测过于频繁

**解决方案:**
```toml
# 减少并发操作数
max_concurrent_ops = 1

# 增加心跳间隔
heartbeat_interval_sec = 60
```

### CPU 占用过高

**症状:**
- 进程 CPU 占用持续偏高

**可能原因:**
1. 日志级别设置过于详细
2. 心跳检测间隔过短
3. 连接重连频率过高

**解决方案:**
```toml
[logging]
level = "warn"  # 减少日志输出

# 增加心跳间隔
heartbeat_interval_sec = 60
```

### 内存使用过高

**症状:**
- 内存占用持续增长

**可能原因:**
1. 日志缓冲区累积
2. 事件缓冲区未清理
3. 连接泄漏

**解决方案:**
- 检查日志轮转配置
- 监控 Variables 中的缓冲区大小
- 检查连接是否正确关闭

## 配置问题

### 配置文件加载失败

**症状:**
- 启动时报错 "Failed to load config"
- 配置解析错误

**可能原因:**
1. TOML 格式错误
2. 缺少必填字段
3. 字段类型错误
4. 信号组验证失败

**解决方案:**
```bash
# 验证 TOML 格式
cat config.toml | python3 -c "import toml, sys; toml.load(sys.stdin)"

# 检查必填字段
# - server.rpc_port
# - server.http_port
# - logging.level
# - logging.file
# - logging.daily_rotation
```

### 配置重载不生效

**症状:**
- 修改配置后未生效
- 日志未显示重载信息

**可能原因:**
1. 配置文件路径错误
2. 文件变更未被检测到
3. 配置内容未改变（ConfigLoader 会对比内容）

**解决方案:**
```bash
# 手动触发重载
curl -X POST http://localhost:8081/api/config/reload

# 检查日志确认重载
# 确保文件权限正确
```

### 信号组验证失败

**症状:**
- 启动报错 "Signal group validation error"

**可能原因:**
1. 字段名称重复
2. 字段偏移量超出范围
3. 地址格式错误
4. 地址超出 0-65535 范围

**解决方案:**
- 检查字段名称唯一性
- 验证 offset + required_registers ≤ register_count
- 检查 register_address 格式（h/i/c/d 前缀）

## API 问题

### JSON-RPC 调用无响应

**症状:**
- 请求挂起不返回
- 连接超时

**可能原因:**
1. RpcWorker 未启动
2. 消息路由问题
3. ModbusWorker 未连接
4. 请求超时（默认 30 秒）

**解决方案:**
```bash
# 检查服务状态
curl http://localhost:8081/api/health

# 检查设备状态
curl http://localhost:8081/api/devices

# 查看日志确认 Worker 启动
tail -f /var/log/roboplc-middleware.log
```

### JSON-RPC 返回错误

**症状:**
- 返回错误响应

**可能原因:**
1. 设备未找到
2. 信号组不存在
3. Modbus 操作失败
4. 参数格式错误

**解决方案:**
- 检查 device_id 是否正确
- 检查 group_name 是否存在
- 检查设备连接状态
- 验证参数格式

### HTTP API 返回 404

**症状:**
- API 端点返回 404

**可能原因:**
1. URL 路径错误
2. HTTP 服务未启动
3. 端口配置错误

**解决方案:**
```bash
# 确认正确的 API 路径
curl http://localhost:8081/api/devices
curl http://localhost:8081/api/health
curl http://localhost:8081/api/config

# 检查端口监听
netstat -tlnp | grep 8081
```

### HTTP API 返回 500

**症状:**
- 内部服务器错误

**可能原因:**
1. 共享状态访问错误
2. 配置序列化失败
3. 内部错误

**解决方案:**
- 查看日志获取详细错误信息
- 检查配置是否完整
- 重启服务

## 消息系统问题

### 消息未到达

**症状:**
- 请求发送后无响应
- DeviceManager 未收到消息

**可能原因:**
1. Hub 未正确初始化
2. Worker 未订阅正确消息类型
3. 消息被过滤

**解决方案:**
- 检查 Worker 的消息订阅配置
- 确认 event_matches 模式正确
- 查看日志确认消息发送

### 消息循环（已修复）

**注意:** 此问题已在架构演进中修复。

旧架构中 DeviceManager 订阅并转发 DeviceControl 消息导致循环。
新架构使用直接响应机制，ModbusWorker 直接响应 RpcWorker。

## Worker 问题

### ModbusWorker 崩溃

**症状:**
- ModbusWorker 异常退出
- 设备连接断开

**可能原因:**
1. 连接异常
2. 内存错误
3. 协议错误

**解决方案:**
- 查看日志获取错误信息
- 检查设备 Modbus 协议兼容性
- 使用 Mock Modbus 测试

### HeartbeatWorker 检测不到设备

**症状:**
- 设备实际在线但显示离线
- 心跳超时

**可能原因:**
1. 设备响应慢
2. 网络延迟
3. GetStatus 操作失败

**解决方案:**
- 增加心跳超时时间
- 检查网络质量
- 验证 ModbusWorker 连接正常

## 日志分析

### 常见日志信息

| 日志信息 | 含义 | 处理方式 |
|---------|------|---------|
| `Connection established` | 设备连接成功 | 正常 |
| `Connection lost` | 连接断开 | 检查网络和设备 |
| `Reconnecting...` | 正在重连 | 正常行为 |
| `Transaction ID mismatch` | 响应 ID 不匹配 | 可能是网络问题 |
| `Latency anomaly detected` | 延迟异常 | 检查网络和设备负载 |
| `Config reloaded` | 配置重载成功 | 正常 |
| `Request timed out` | 请求超时 | 检查设备和网络 |
| `Invalid signal group` | 信号组不存在 | 检查配置 |

### 启用详细日志

```toml
[logging]
level = "debug"  # 或 "trace" 获取最详细日志
```

### 日志轮转

```toml
[logging]
file = "/var/log/roboplc-middleware.log"
daily_rotation = true  # 按天轮转
```

## 调试技巧

### 使用 Mock Modbus 测试

```bash
# 启动 Mock 服务器
cargo run --bin mock_server

# 运行测试
cargo test --test e2e_tests
```

### 检查内部状态

```bash
# 查看所有设备状态
curl http://localhost:8081/api/devices | jq

# 查看健康状态
curl http://localhost:8081/api/health | jq

# 查看当前配置
curl http://localhost:8081/api/config | jq
```

### 实时监控

```bash
# 监控日志
tail -f /var/log/roboplc-middleware.log

# 监控网络连接
watch -n 1 'netstat -an | grep 502'

# 监控进程资源使用
top -p $(pgrep roboplc-middleware)
```

## 获取帮助

如果以上方法无法解决问题：

1. 检查日志文件获取详细错误信息
2. 使用 `ROBOPLC_SIMULATED=1` 环境变量跳过实时调度进行测试
3. 使用 Mock Modbus 服务器隔离问题
4. 提交 Issue 并附上：
   - 配置文件（脱敏）
   - 错误日志
   - 复现步骤
   - 系统环境信息（OS、Rust 版本等）
