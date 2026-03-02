# 集成部署指南

本文档说明 roboplc-middleware 的部署、监控和集成方式。

## 系统要求

- 操作系统: Linux (推荐 Ubuntu 20.04+)
- Rust: 1.70+ (用于编译)
- 网络支持: TCP/IP

## 编译安装

### 从源码编译

```bash
# 克隆代码库
git clone <repository-url>
cd roboplc-middleware

# 编译发布版本
cargo build --release

# 编译结果位于 target/release/roboplc-middleware
```

### 运行测试

```bash
# 运行所有测试
cargo test

# 运行特定测试
cargo test test_worker_creation_logic
```

## 部署方式

### 直接运行

```bash
# 创建配置文件
cp config.sample.toml config.toml
# 编辑配置文件

# 运行（需要 root 权限进行 RT 调度）
sudo ./target/release/roboplc-middleware

# 开发模式（跳过 RT 调度）
ROBOPLC_SIMULATED=1 ./target/release/roboplc-middleware
```

### systemd 服务

创建服务文件 `/etc/systemd/system/roboplc-middleware.service`:

```ini
[Unit]
Description=RoboPLC Communication Middleware
After=network.target

[Service]
Type=simple
User=root
WorkingDirectory=/opt/roboplc-middleware
ExecStart=/opt/roboplc-middleware/roboplc-middleware
Restart=on-failure
RestartSec=5s

# 实时调度配置
LimitRTPRIO=90
LimitMEMLOCK=infinity

[Install]
WantedBy=multi-user.target
```

启用服务:

```bash
sudo systemctl daemon-reload
sudo systemctl enable roboplc-middleware
sudo systemctl start roboplc-middleware
sudo systemctl status roboplc-middleware
```

### Docker 部署

创建 `Dockerfile`:

```dockerfile
FROM rust:1.75 as builder
WORKDIR /app
COPY . .
RUN cargo build --release

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y libssl3 ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/roboplc-middleware /usr/local/bin/
COPY config.toml /etc/roboplc-middleware/
WORKDIR /etc/roboplc-middleware
CMD ["roboplc-middleware"]
```

构建和运行:

```bash
docker build -t roboplc-middleware .
docker run -d \
  --name middleware \
  --network host \
  -v /path/to/config.toml:/etc/roboplc-middleware/config.toml \
  roboplc-middleware
```

## 配置集成

### 与上位软件集成

上位软件通过 JSON-RPC 2.0 协议与中间件通信。

**Python 示例:**

```python
import requests
import json

def call_rpc(method, params=None):
    url = "http://localhost:8080/jsonrpc"
    payload = {
        "jsonrpc": "2.0",
        "method": method,
        "params": params or [],
        "id": 1
    }
    response = requests.post(url, json=payload)
    return response.json()

# 获取设备列表
result = call_rpc("get_device_list")
print(result)

# 读取寄存器
result = call_rpc("get_register", ["plc-1", "h100"])
print(result)

# 写入寄存器
result = call_rpc("set_register", ["plc-1", "h100", 42])
print(result)
```

**C# 示例:**

```csharp
using System.Net.Http;
using System.Text;
using Newtonsoft.Json;

public class RpcClient
{
    private readonly HttpClient _client = new HttpClient();
    private readonly string _url = "http://localhost:8080/jsonrpc";

    public async Task<T> CallAsync<T>(string method, params object[] args)
    {
        var payload = new
        {
            jsonrpc = "2.0",
            method = method,
            @params = args,
            id = 1
        };
        
        var content = new StringContent(
            JsonConvert.SerializeObject(payload),
            Encoding.UTF8,
            "application/json"
        );
        
        var response = await _client.PostAsync(_url, content);
        var result = JsonConvert.DeserializeObject<dynamic>(
            await response.Content.ReadAsStringAsync()
        );
        
        return result.result.ToObject<T>();
    }
}
```

### 与 PLC 集成

配置 Modbus TCP 连接参数:

```toml
[[devices]]
id = "siemens-plc"
type = "plc"
address = "192.168.1.10"
port = 502
unit_id = 1
addressing_mode = "zero_based"
byte_order = "big_endian"

[[devices.register_mappings]]
signal_name = "motor_speed"
address = "h100"
data_type = "U16"
```

### 与机械臂集成

```toml
[[devices]]
id = "robot-arm"
type = "robot_arm"
address = "192.168.1.20"
port = 502
unit_id = 1

[[devices.register_mappings]]
signal_name = "position_x"
address = "h0"
data_type = "F32"
```

## 监控

### HTTP 健康检查

```bash
# 健康检查
curl http://localhost:8081/api/health

# 设备状态
curl http://localhost:8081/api/devices

# 特定设备状态
curl http://localhost:8081/api/devices/plc-1/status
```

### 日志监控

日志文件位置（配置中指定）:
```
/var/log/roboplc-middleware.log
```

实时查看日志:
```bash
tail -f /var/log/roboplc-middleware.log
```

### Prometheus 集成（预留）

中间件预留了指标导出接口，可通过以下方式集成:

1. 添加 Prometheus exporter
2. 配置指标采集端点
3. 在 Prometheus 配置中添加 scrape target

## 性能调优

### 实时调度

对于需要低延迟的场景，确保启用实时调度:

```bash
# 检查实时调度权限
ulimit -r

# 设置实时调度优先级
sudo setcap cap_sys_nice+ep ./target/release/roboplc-middleware
```

### 网络优化

```bash
# 增加网络缓冲区
sudo sysctl -w net.core.rmem_max=16777216
sudo sysctl -w net.core.wmem_max=16777216
```

### 并发配置

根据设备处理能力调整:

```toml
[[devices]]
max_concurrent_ops = 5  # 设备支持的并发操作数
```

## 故障恢复

### 自动重连

中间件内置自动重连机制:
- 指数退避重连策略
- 抖动防止雷群效应
- 连接状态追踪

### 优雅关闭

```bash
# 发送终止信号
sudo systemctl stop roboplc-middleware

# 或使用 SIGTERM
kill -TERM <pid>
```

中间件会:
1. 停止接受新请求
2. 完成进行中的请求
3. 关闭所有连接
4. 退出（默认 5 秒超时）

## 安全建议

1. **网络隔离**: 将中间件部署在隔离网络中
2. **防火墙**: 仅开放必要端口
3. **访问控制**: 在反向代理层添加认证
4. **日志审计**: 定期检查日志