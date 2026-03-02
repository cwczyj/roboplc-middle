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
```

### 连接建立后立即断开

**症状:**
- 连接成功但立即断开
- 日志显示频繁的 "Connected" 和 "Disconnected"

**可能原因:**
1. `tcp_nodelay` 设置与设备不兼容
2. 设备不支持多连接
3. 连接超时设置过短

**解决方案:**
```toml
# 尝试禁用 tcp_nodelay
[[devices]]
tcp_nodelay = false

# 增加心跳间隔
heartbeat_interval_sec = 60
```

## 数据读取问题

### 读取的值不正确

**症状:**
- 数值与预期不符
- 数值出现异常大的值或负值

**可能原因:**
1. 字节序设置错误
2. 地址模式设置错误
3. 数据类型不匹配

**解决方案:**

**检查字节序:**
```toml
# 尝试不同的字节序
byte_order = "little_endian"  # 或 "big_endian"
```

**检查地址模式:**
```toml
# 如果读取的地址偏移 1，尝试切换地址模式
addressing_mode = "one_based"  # 或 "zero_based"
```

### 读取寄存器返回错误

**症状:**
- Modbus 异常码错误
- 部分地址读取失败

**可能原因:**
1. 地址超出设备范围
2. 设备不支持该功能码
3. 单元 ID 错误

**解决方案:**
```bash
# 使用 modpoll 工具测试
modpoll -m tcp -t 3 -r 100 -c 10 192.168.1.100
```

## 性能问题

### 响应延迟过高

**症状:**
- 响应时间超过预期
- 偶发超时

**可能原因:**
1. 并发操作过多
2. 网络延迟
3. 设备处理能力不足

**解决方案:**
```toml
# 减少并发操作数
max_concurrent_ops = 1

# 增加超时时间（在代码中调整）
```

### CPU 占用过高

**症状:**
- 进程 CPU 占用持续偏高

**可能原因:**
1. 日志级别设置过于详细
2. 心跳检测间隔过短

**解决方案:**
```toml
[logging]
level = "warn"  # 减少日志输出

# 增加心跳间隔
heartbeat_interval_sec = 60
```

## 配置问题

### 配置文件加载失败

**症状:**
- 启动时报错 "Failed to load config"
- 配置解析错误

**可能原因:**
1. TOML 格式错误
2. 缺少必填字段
3. 字段类型错误

**解决方案:**
```bash
# 验证 TOML 格式
cat config.toml | python3 -c "import toml, sys; toml.load(sys.stdin)"
```

### 配置重载不生效

**症状:**
- 修改配置后未生效
- 日志未显示重载信息

**可能原因:**
1. 配置文件路径错误
2. 文件变更未被检测到

**解决方案:**
```bash
# 手动触发重载
curl -X POST http://localhost:8081/api/config/reload
```

## API 问题

### JSON-RPC 调用无响应

**症状:**
- 请求挂起不返回
- 连接超时

**可能原因:**
1. Worker 未启动
2. 消息路由问题

**解决方案:**
```bash
# 检查服务状态
curl http://localhost:8081/api/health

# 检查设备状态
curl http://localhost:8081/api/devices
```

### HTTP API 返回 404

**症状:**
- API 端点返回 404

**可能原因:**
1. URL 路径错误
2. HTTP 服务未启动

**解决方案:**
```bash
# 确认正确的 API 路径
curl http://localhost:8081/api/devices
curl http://localhost:8081/api/health
```

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

### 启用详细日志

```toml
[logging]
level = "debug"  # 或 "trace" 获取最详细日志
```

## 获取帮助

如果以上方法无法解决问题：

1. 检查日志文件获取详细错误信息
2. 使用 `ROBOPLC_SIMULATED=1` 环境变量跳过实时调度进行测试
3. 提交 Issue 并附上：
   - 配置文件（脱敏）
   - 错误日志
   - 复现步骤