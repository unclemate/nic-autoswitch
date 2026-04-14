# nic-autoswitch

[![Rust](https://img.shields.io/badge/rust-1.75%2B-orange.svg)](https://www.rust-lang.org/)
[![License](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

Linux 网络自动分流守护进程，根据网络接口状态智能管理流量路由。

## 功能特性

- **多网卡智能路由** - LAN/WLAN 环境下根据目标地址选择最优路径
- **企业网络优化** - 内网服务走内网，外部流量走公网
- **网络切换自动化** - WiFi/有线变化时自动调整路由规则
- **IPv4/IPv6 双栈支持** - 完整的双栈路由管理
- **WiFi 配置文件** - 根据 SSID 自动切换路由策略
- **热加载配置** - 无需重启即可更新配置
- **systemd 集成** - 原生支持 systemd 服务管理

## 系统要求

- Linux 内核 4.4+
- Rust 1.75+
- systemd (可选，用于服务管理)
- NetworkManager (可选，用于 WiFi SSID 监控)
- root 权限 (操作路由表需要 `CAP_NET_ADMIN`)

## 安装

### 从源码编译

```bash
# 克隆仓库
git clone https://github.com/unclemt/nic-autoswitch.git
cd nic-autoswitch

# 编译
cargo build --release

# 安装
sudo make install
```

### 安装 systemd 服务

```bash
sudo cp systemd/nic-autoswitch.service /etc/systemd/system/
sudo systemctl daemon-reload
sudo systemctl enable --now nic-autoswitch
```

## 配置

配置文件默认位于 `/etc/nic-autoswitch/config.toml`。

### 示例配置

```toml
[global]
monitor_interval = 5      # 监控间隔（秒）
log_level = "info"        # 日志级别: trace, debug, info, warn, error
dry_run = false           # 干跑模式，只打印不执行
table_id_start = 100      # 路由表 ID 起始值

# 网络接口配置
[interfaces.eth0]
interface_type = "lan"    # 接口类型: lan, wlan
match_by = { name = "eth0" }
priority = 10             # 优先级，数字越小越优先

[interfaces.wlan0]
interface_type = "wlan"
match_by = { name = "wlan0" }
priority = 20

# 默认路由规则
[[routing.default_rules]]
name = "default-lan"
match_on = { cidr = "10.0.0.0/8" }
route_via = { interface = "eth0" }
priority = 100

[[routing.default_rules]]
name = "default-internet"
match_on = { cidr = "0.0.0.0/0" }
route_via = { interface = "wlan0" }
priority = 10000

# WiFi 配置文件（可选）
[wifi_profiles."CorpWiFi"]
interface = "wlan0"

[[wifi_profiles."CorpWiFi".rules]]
name = "corp-intranet"
match_on = { cidr = "192.168.0.0/16" }
route_via = { interface = "eth0" }
priority = 50
```

### 匹配规则类型

| 类型 | 语法 | 示例 |
|------|------|------|
| CIDR | `{ cidr = "x.x.x.x/y" }` | `{ cidr = "10.0.0.0/8" }` |
| 单 IP | `{ ip = "x.x.x.x" }` | `{ ip = "192.168.1.1" }` |
| 域名 | `{ domain = "example.com" }` | `{ domain = "internal.corp.com" }` |
| 通配符域名 | `{ domain_pattern = "*.example.com" }` | `{ domain_pattern = "*.corp.local" }` |
| 通配符域名（前缀） | `{ domain_pattern = "heals-*" }` | `{ domain_pattern = "app-*" }` |

## CLI 工具

`nic-autoswitch-cli` 用于与运行中的守护进程交互：

```bash
# 查看状态
nic-autoswitch-cli status

# 查看活跃路由
nic-autoswitch-cli routes

# 重载配置
nic-autoswitch-cli reload

# 检查守护进程
nic-autoswitch-cli ping

# 请求关闭
nic-autoswitch-cli shutdown

# 指定 socket 路径
nic-autoswitch-cli -s /run/nic-autoswitch/control.sock status
```

## 架构概览

```
┌─────────────────────────────────────────────────────────────┐
│                        Daemon Service                        │
├─────────────────────────────────────────────────────────────┤
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────────────┐  │
│  │  Signal     │  │  Config     │  │  Unix Socket        │  │
│  │  Handler    │  │  Loader     │  │  Control Server     │  │
│  └─────────────┘  └─────────────┘  └─────────────────────┘  │
├─────────────────────────────────────────────────────────────┤
│                     Event Dispatcher                         │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────────────┐  │
│  │  Rule       │  │  Network    │  │  Route              │  │
│  │  Matcher    │  │  State      │  │  Executor           │  │
│  └─────────────┘  └─────────────┘  └─────────────────────┘  │
├─────────────────────────────────────────────────────────────┤
│  ┌─────────────────────┐  ┌─────────────────────────────┐   │
│  │     Monitor Layer   │  │      Router Layer           │   │
│  │  ┌───────┐ ┌──────┐ │  │  ┌────────┐ ┌────────────┐  │   │
│  │  │Netlink│ │D-Bus │ │  │  │Manager │ │  Operator  │  │   │
│  │  └───────┘ └──────┘ │  │  └────────┘ └────────────┘  │   │
│  └─────────────────────┘  └─────────────────────────────┘   │
└─────────────────────────────────────────────────────────────┘
```

### 模块说明

| 模块 | 功能 |
|------|------|
| `config` | 配置加载、验证、热加载 |
| `monitor` | 网络事件监控 (rtnetlink + D-Bus) |
| `router` | 路由表管理、规则操作、DNS 解析 |
| `engine` | 事件分发、规则匹配、执行 |
| `daemon` | 服务管理、信号处理、systemd 集成 |
| `cli` | 命令行工具 |

## 开发

### 环境准备

```bash
# 安装 Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# 安装 pre-commit hooks
pre-commit install
```

### 常用命令

```bash
# 格式化代码
cargo fmt

# 静态检查
cargo clippy -- -D warnings

# 运行测试
cargo test

# 运行所有检查
cargo fmt && cargo clippy -- -D warnings && cargo test

# 生成文档
cargo doc --open

# 运行基准测试
cargo bench

# 测试覆盖率
cargo tarpaulin --out Html
```

### 测试

```bash
# 运行所有测试
cargo test --all

# 运行特定测试
cargo test test_load_valid_config

# 运行集成测试
cargo test --test integration_config
cargo test --test integration_engine
```

## 路由表 ID 分配

守护进程使用策略路由，自动分配路由表 ID：

| 范围 | 用途 |
|------|------|
| 100-199 | IPv4 路由规则 |
| 200-299 | IPv6 路由规则 |

每个规则优先级对应一个独立的路由表，ID = `table_id_start` + `priority`。

## 故障排除

### 查看日志

```bash
# systemd 日志
journalctl -u nic-autoswitch -f

# 查看最近 100 行
journalctl -u nic-autoswitch -n 100
```

### 常见问题

1. **权限不足**
   ```
   Error: Permission denied (os error 13)
   ```
   解决：使用 root 权限运行或授予 `CAP_NET_ADMIN` capability。

2. **配置文件不存在**
   ```
   Error: Config file not found
   ```
   解决：创建 `/etc/nic-autoswitch/config.toml` 配置文件。

3. **路由表 ID 冲突**
   ```
   Error: Table ID already in use
   ```
   解决：调整 `table_id_start` 配置，避免与现有路由表冲突。

## 许可证

MIT License. 详见 [LICENSE](LICENSE) 文件。

## 贡献

欢迎提交 Issue 和 Pull Request！

1. Fork 本仓库
2. 创建特性分支 (`git checkout -b feature/amazing-feature`)
3. 提交更改 (`git commit -m 'feat: add amazing feature'`)
4. 推送到分支 (`git push origin feature/amazing-feature`)
5. 创建 Pull Request
