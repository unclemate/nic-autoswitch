#!/bin/bash
# deploy-local.sh — 本机部署 nic-autoswitch
#
# 用法: sudo bash scripts/deploy-local.sh
#
# 包含: 二进制安装、配置文件、日志方案、systemd 服务

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"

echo "=========================================="
echo " nic-autoswitch 本机部署"
echo "=========================================="
echo ""

# ---- Step 1: 检查构建产物 ----
echo "[1/8] 检查构建产物..."
for bin in nic-autoswitch nic-autoswitch-cli; do
    if [ ! -f "$PROJECT_DIR/target/release/$bin" ]; then
        echo "ERROR: $bin 未找到，请先运行 cargo build --release"
        exit 1
    fi
done
echo "  ✓ 二进制文件就绪"

# ---- Step 2: 安装二进制 ----
echo "[2/8] 安装二进制到 /usr/local/bin/..."
cp "$PROJECT_DIR/target/release/nic-autoswitch" /usr/local/bin/
cp "$PROJECT_DIR/target/release/nic-autoswitch-cli" /usr/local/bin/
chmod 755 /usr/local/bin/nic-autoswitch /usr/local/bin/nic-autoswitch-cli
echo "  ✓ /usr/local/bin/nic-autoswitch"
echo "  ✓ /usr/local/bin/nic-autoswitch-cli"

# ---- Step 3: 创建配置目录和配置文件 ----
echo "[3/8] 创建配置文件..."
mkdir -p /etc/nic-autoswitch

if [ -f /etc/nic-autoswitch/config.toml ]; then
    echo "  ⚠ 配置文件已存在，备份为 config.toml.bak.$(date +%Y%m%d%H%M%S)"
    cp /etc/nic-autoswitch/config.toml "/etc/nic-autoswitch/config.toml.bak.$(date +%Y%m%d%H%M%S)"
fi

cat > /etc/nic-autoswitch/config.toml << 'CONF'
# nic-autoswitch 本机配置
# WiFi + 有线分流

[global]
monitor_interval = 5
log_level = "info"
dry_run = false
table_id_start = 100

# 内置有线网卡
[interfaces.enp0s31f6]
interface_type = "lan"
match_by = { name = "enp0s31f6" }
priority = 10

# USB 有线网卡
[interfaces.enp58s0u1u1]
interface_type = "lan"
match_by = { name = "enp58s0u1u1" }
priority = 15

# WiFi 接口
[interfaces.wlan0]
interface_type = "wlan"
match_by = { name = "wlan0" }
priority = 20

# 有线分流规则：192.168.99.0/24 走有线
[[routing.default_rules]]
name = "lan-subnet"
match_on = { cidr = "192.168.99.0/24" }
route_via = { interface = "enp0s31f6" }
priority = 100

# 默认路由走 WiFi
[[routing.default_rules]]
name = "default-wifi"
match_on = { cidr = "0.0.0.0/0" }
route_via = { interface = "wlan0" }
priority = 10000

[[routing.default_rules]]
name = "default-ipv6"
match_on = { cidr = "::/0" }
route_via = { interface = "wlan0" }
priority = 10001
CONF

echo "  ✓ /etc/nic-autoswitch/config.toml"

# ---- Step 4: 配置 journald 持久化 ----
echo "[4/8] 配置 journald 持久化..."
mkdir -p /etc/systemd/journald.conf.d

cat > /etc/systemd/journald.conf.d/nic-autoswitch.conf << 'JCONF'
[Journal]
Storage=persistent
SystemMaxUse=50M
SystemMaxFileSize=10M
MaxRetentionSec=7day
ForwardToConsole=no
JCONF

echo "  ✓ /etc/systemd/journald.conf.d/nic-autoswitch.conf"

# ---- Step 5: 配置日志目录和 logrotate ----
echo "[5/8] 配置日志文件和滚动..."
mkdir -p /var/log/nic-autoswitch
chown root:adm /var/log/nic-autoswitch
chmod 2750 /var/log/nic-autoswitch

cat > /etc/logrotate.d/nic-autoswitch << 'LRCONF'
/var/log/nic-autoswitch/*.log {
    daily
    missingok
    rotate 7
    compress
    delaycompress
    notifempty
    maxsize 10M
    create 0640 root adm
    sharedscripts
    postrotate
        systemctl reload nic-autoswitch > /dev/null 2>&1 || true
    endscript
}
LRCONF

echo "  ✓ /var/log/nic-autoswitch/"
echo "  ✓ /etc/logrotate.d/nic-autoswitch"

# ---- Step 6: 安装 systemd 服务 ----
echo "[6/8] 安装 systemd 服务..."

cat > /etc/systemd/system/nic-autoswitch.service << 'SVC'
[Unit]
Description=NIC Auto-Switch Daemon
Documentation=https://github.com/unclemt/nic-autoswitch
After=network.target
ConditionPathExists=/etc/nic-autoswitch/config.toml

[Service]
Type=notify
NotifyAccess=all
ExecStart=/usr/local/bin/nic-autoswitch --config /etc/nic-autoswitch/config.toml
ExecReload=/bin/kill -HUP $MAINPID
Restart=on-failure
RestartSec=5

# Watchdog
WatchdogSec=30

# 日志输出：journald + 文件
StandardOutput=journal+append:/var/log/nic-autoswitch/nic-autoswitch.log
StandardError=inherit

# Security
NoNewPrivileges=yes
PrivateTmp=yes
ProtectSystem=yes
RestrictAddressFamilies=AF_INET AF_INET6 AF_NETLINK AF_UNIX
RestrictRealtime=yes
RestrictSUIDSGID=yes

# Resource limits
LimitNOFILE=65536
TimeoutStopSec=30

[Install]
WantedBy=multi-user.target
SVC

systemctl daemon-reload
echo "  ✓ /etc/systemd/system/nic-autoswitch.service"

# ---- Step 7: 验证 ----
echo "[7/8] 验证安装..."
echo "  二进制:"
echo "    $(which nic-autoswitch)"
echo "    $(which nic-autoswitch-cli)"
echo "  配置:"
echo "    $(ls -la /etc/nic-autoswitch/config.toml)"
echo "  日志:"
echo "    $(ls -la /var/log/nic-autoswitch/)"
echo "  服务:"
systemctl cat nic-autoswitch.service | head -3

# ---- Step 8: 启动 ----
echo ""
echo "[8/8] 启动服务..."
echo ""
echo "  部署完成！下一步操作："
echo ""
echo "  # 首次验证（dry-run 模式）"
echo "  sudo nic-autoswitch -f --dry-run -l debug"
echo ""
echo "  # 确认无误后启动服务"
echo "  sudo systemctl enable --now nic-autoswitch"
echo ""
echo "  # 查看状态"
echo "  sudo systemctl status nic-autoswitch"
echo ""
echo "  # 查看日志"
echo "  journalctl -u nic-autoswitch -f"
echo "  tail -f /var/log/nic-autoswitch/nic-autoswitch.log"
echo ""
echo "  # CLI 工具"
echo "  nic-autoswitch-cli status"
echo "  nic-autoswitch-cli routes"
echo ""
