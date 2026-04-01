#!/bin/bash
# 容器内模拟多网卡环境
#
# 用法: setup-netns.sh [--] [command...]
#   无参数: 创建接口后进入 bash
#   -- <cmd>: 创建接口后执行 <cmd>

set -e

EXEC_CMD=()

# 解析参数
while [[ $# -gt 0 ]]; do
    case $1 in
        --) shift; EXEC_CMD=("$@"); break ;;
        *)  EXEC_CMD=("$@"); break ;;
    esac
done

echo "[setup-netns] Creating simulated network interfaces..."

# dummy 接口模拟 LAN（命名 nic0 避免与 Docker 自身 eth0 冲突）
ip link add nic0 type dummy 2>/dev/null || true
ip link set nic0 up
ip addr add 192.168.1.100/24 dev nic0 2>/dev/null || true

# dummy 接口模拟 WLAN
ip link add nic1 type dummy 2>/dev/null || true
ip link set nic1 up
ip addr add 192.168.2.100/24 dev nic1 2>/dev/null || true

# dummy 接口模拟 VPN
ip link add nic2 type dummy 2>/dev/null || true
ip link set nic2 up
ip addr add 10.8.0.1/24 dev nic2 2>/dev/null || true

# 注册路由表 ID 到 iproute2
if [ -f /etc/iproute2/rt_tables ]; then
    for i in $(seq 100 120); do
        grep -qw "^${i}" /etc/iproute2/rt_tables 2>/dev/null || \
            echo "${i} table${i}" >> /etc/iproute2/rt_tables
    done
fi

echo "[setup-netns] Interfaces ready:"
ip -br addr
echo ""

if [[ ${#EXEC_CMD[@]} -gt 0 ]]; then
    echo "[setup-netns] Executing: ${EXEC_CMD[*]}"
    exec "${EXEC_CMD[@]}"
else
    echo "[setup-netns] Starting interactive shell"
    echo "  cargo build                   - 编译 (debug)"
    echo "  cargo test                    - 运行测试"
    echo "  cargo run -- --dry-run -f     - 干运行模式"
    echo "  ip route show                 - 查看路由表"
    echo ""
    exec /bin/bash
fi
