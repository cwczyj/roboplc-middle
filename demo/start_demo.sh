#!/bin/bash

# Demo 启动脚本 - 用于测试 roboplc-middleware 完整流程
# 
# 用法:
#   ./start_demo.sh           # 启动所有服务
#   ./start_demo.sh mock      # 只启动 Mock Server
#   ./start_demo.sh client    # 只启动 JSON-RPC 客户端
#   ./start_demo.sh help      # 显示帮助

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"
CONFIG_FILE="$PROJECT_ROOT/config.toml"
MOCK_CONFIG="$SCRIPT_DIR/config-mock.toml"

# 颜色定义
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

print_help() {
    echo "Demo 启动脚本 - roboplc-middleware"
    echo ""
    echo "用法:"
    echo "  $0 [command]"
    echo ""
    echo "命令:"
    echo "  (无参数)     依次启动 Mock Server 和 Middleware"
    echo "  mock         只启动 Mock Modbus Server"
    echo "  middleware   只启动 Middleware (需要先复制配置文件)"
    echo "  client       运行 JSON-RPC 客户端示例"
    echo "  setup        复制配置文件到项目根目录"
    echo "  help         显示此帮助信息"
    echo ""
    echo "完整启动流程:"
    echo "  1. 终端 1: cargo run --bin mock_server"
    echo "  2. 终端 2: ROBOPLC_SIMULATED=1 cargo run"
    echo "  3. 终端 3: cargo run --bin jsonrpc_client -- read motor_control"
    echo ""
}

setup_config() {
    echo -e "${BLUE}正在复制配置文件...${NC}"
    cp "$MOCK_CONFIG" "$CONFIG_FILE"
    echo -e "${GREEN}✓ 配置文件已复制到：$CONFIG_FILE${NC}"
}

start_mock_server() {
    echo -e "${BLUE}启动 Mock Modbus Server...${NC}"
    echo -e "${YELLOW}提示：按 Ctrl+C 停止服务器${NC}"
    echo ""
    cd "$PROJECT_ROOT"
    cargo run --bin mock_server
}

start_middleware() {
    echo -e "${BLUE}启动 roboplc-middleware...${NC}"
    echo -e "${YELLOW}提示：按 Ctrl+C 停止服务${NC}"
    echo ""
    cd "$PROJECT_ROOT"
    ROBOPLC_SIMULATED=1 cargo run --bin roboplc-middleware
}

run_client() {
    echo -e "${BLUE}运行 JSON-RPC 客户端...${NC}"
    echo ""
    cd "$PROJECT_ROOT"
    cargo run --bin jsonrpc_client "$@"
}

# 主逻辑
case "${1:-}" in
    "help"|"--help"|"-h")
        print_help
        ;;
    "mock")
        start_mock_server
        ;;
    "middleware")
        if [ ! -f "$CONFIG_FILE" ]; then
            echo -e "${YELLOW}配置文件不存在，正在设置...${NC}"
            setup_config
        fi
        start_middleware
        ;;
    "client")
        shift
        run_client "$@"
        ;;
    "setup")
        setup_config
        ;;
    "")
        # 无参数：显示引导信息
        echo -e "${GREEN}========================================${NC}"
        echo -e "${GREEN}  roboplc-middleware Demo${NC}"
        echo -e "${GREEN}========================================${NC}"
        echo ""
        echo "这是一个完整的通信中间件演示系统。"
        echo ""
        echo "启动步骤:"
        echo ""
        echo -e "${BLUE}步骤 1: 设置配置文件${NC}"
        echo "  $0 setup"
        echo ""
        echo -e "${BLUE}步骤 2: 启动 Mock Modbus Server (终端 1)${NC}"
        echo "  $0 mock"
        echo ""
        echo -e "${BLUE}步骤 3: 启动 Middleware (终端 2)${NC}"
        echo "  $0 middleware"
        echo ""
        echo -e "${BLUE}步骤 4: 测试通信 (终端 3)${NC}"
        echo "  $0 client read motor_control"
        echo "  $0 client write motor_control motor_speed 2000"
        echo ""
        echo -e "${YELLOW}提示：每个服务需要在单独的终端窗口中运行${NC}"
        echo ""
        echo "查看完整文档：cat demo/README.md"
        ;;
    *)
        echo -e "${RED}未知命令：$1${NC}"
        echo ""
        print_help
        exit 1
        ;;
esac