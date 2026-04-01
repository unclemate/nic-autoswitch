# INSTRUCTION.md — nic-autoswitch 使用指南

## 目录

- [1. 项目概述](#1-项目概述)
- [2. 系统要求](#2-系统要求)
- [3. 安装](#3-安装)
- [4. 配置](#4-配置)
- [5. 运行](#5-运行)
- [6. CLI 工具](#6-cli-工具)
- [7. Docker 开发环境](#7-docker-开发环境)
- [8. 测试](#8-测试)
- [9. 开发工作流](#9-开发工作流)
- [10. 故障排除](#10-故障排除)

---

## 1. 项目概述

nic-autoswitch 是一个 Linux 网络自动分流守护进程。它根据网络接口状态（上线/下线、WiFi 连接/断开、地址变更等）自动管理策略路由规则，实现：

- **多网卡智能路由**：LAN/WLAN 环境下根据目标地址选择最优路径
- **企业网络优化**：公司内网流量走有线/VPN，外部流量走 WiFi
- **WiFi 配置文件**：根据当前连接的 SSID 自动切换路由策略
- **配置热加载**：修改配置文件后自动生效，无需重启
- **IPv4/IPv6 双栈**：完整支持双栈路由管理

---

## 2. 系统要求

| 项目 | 要求 |
|------|------|
| 操作系统 | Linux 内核 4.4+ |
| Rust | 1.75+（edition 2024） |
| 权限 | `root` 或 `CAP_NET_ADMIN` capability |
| systemd | 可选，用于服务管理 |
| NetworkManager | 可选，用于 WiFi SSID 监控（无 NM 时降级为仅 netlink 模式） |

---

## 3. 安装

### 3.1 从源码编译

```bash
git clone https://github.com/unclemt/nic-autoswitch.git
cd nic-autoswitch
cargo build --release
```

编译产物位于 `target/release/`：

| 文件 | 用途 |
|------|------|
| `nic-autoswitch` | 守护进程主程序 |
| `nic-autoswitch-cli` | 命令行管理工具 |

### 3.2 系统安装

```bash
# 复制二进制文件
sudo cp target/release/nic-autoswitch /usr/bin/
sudo cp target/release/nic-autoswitch-cli /usr/bin/

# 创建配置目录
sudo mkdir -p /etc/nic-autoswitch

# 复制示例配置
sudo cp config.example.toml /etc/nic-autoswitch/config.toml

# 安装 systemd 服务
sudo cp systemd/nic-autoswitch.service /etc/systemd/system/
sudo systemctl daemon-reload
sudo systemctl enable --now nic-autoswitch
```

### 3.3 权限配置

操作路由表需要 `CAP_NET_ADMIN`：

```bash
# 方式一：直接用 root 运行
sudo nic-autoswitch

# 方式二：授予 capability（无需 root）
sudo setcap cap_net_admin+ep /usr/bin/nic-autoswitch
```

---

## 4. 配置

### 4.1 配置文件位置

默认：`/etc/nic-autoswitch/config.toml`

可通过命令行参数 `-c` / `--config` 指定其他路径。

### 4.2 全局配置 `[global]`

```toml
[global]
monitor_interval = 5       # 监控间隔（秒）
log_level = "info"         # 日志级别: trace, debug, info, warn, error
dry_run = false            # 干运行模式：只打印不实际修改路由表
table_id_start = 100       # 路由表 ID 起始值（有效范围: 100-199）
```

| 参数 | 类型 | 默认值 | 说明 |
|------|------|--------|------|
| `monitor_interval` | u64 | 5 | 网络状态轮询间隔（秒） |
| `log_level` | string | "info" | 日志级别 |
| `dry_run` | bool | false | true 时仅打印操作不实际执行 |
| `table_id_start` | u32 | 100 | IPv4 路由表 ID 基数（100-199） |

### 4.3 网络接口 `[interfaces.<name>]`

```toml
[interfaces.eth0]
interface_type = "lan"          # 接口类型: lan, wlan, vpn
match_by = { name = "eth0" }   # 匹配方式
priority = 10                   # 优先级（数字越小越优先）

[interfaces.wlan0]
interface_type = "wlan"
match_by = { name = "wlan0" }
priority = 20

[interfaces.tun0]
interface_type = "vpn"
match_by = { mac = "aa:bb:cc:dd:ee:ff" }
priority = 5
```

**匹配方式 `match_by`**：

| 方式 | 语法 | 说明 |
|------|------|------|
| 精确名称 | `{ name = "eth0" }` | 按接口名精确匹配 |
| 模式匹配 | `{ pattern = "eth*" }` | 按通配符模式匹配 |
| MAC 地址 | `{ mac = "aa:bb:cc:dd:ee:ff" }` | 按 MAC 地址匹配 |

### 4.4 WiFi 配置文件 `[wifi_profiles."<SSID>"]`

根据当前连接的 WiFi SSID 应用不同的路由规则：

```toml
[wifi_profiles."CorpWiFi"]
interface = "wlan0"             # 关联的 WLAN 接口

[[wifi_profiles."CorpWiFi".rules]]
name = "corp-intranet"
match_on = { cidr = "10.0.0.0/8" }
route_via = { interface = "eth0" }
priority = 100

[[wifi_profiles."CorpWiFi".rules]]
name = "corp-gitlab"
match_on = { domain = "gitlab.corp.com" }
route_via = { interface = "eth0" }
priority = 120

[[wifi_profiles."CorpWiFi".rules]]
name = "corp-wildcard"
match_on = { domain_pattern = "*.corp.example.com" }
route_via = { interface = "eth0" }
priority = 130

[wifi_profiles."HomeWiFi"]
interface = "wlan0"

[[wifi_profiles."HomeWiFi".rules]]
name = "home-nas"
match_on = { cidr = "192.168.1.0/24" }
route_via = { interface = "wlan0" }
priority = 100
```

### 4.5 默认路由规则 `[[routing.default_rules]]`

当没有 WiFi 配置文件匹配时（未连接 WiFi 或 SSID 未配置），应用默认规则：

```toml
[[routing.default_rules]]
name = "default-internet"
match_on = { cidr = "0.0.0.0/0" }
route_via = { interface = "wlan0" }
priority = 10000

[[routing.default_rules]]
name = "default-ipv6"
match_on = { cidr = "::/0" }
route_via = { interface = "wlan0" }
priority = 10001
```

### 4.6 匹配规则类型

| 类型 | 语法 | 说明 | 示例 |
|------|------|------|------|
| CIDR 网段 | `{ cidr = "x.x.x.x/y" }` | 匹配 IP 网段 | `10.0.0.0/8` |
| 单 IP 地址 | `{ ip = "x.x.x.x" }` | 匹配单个 IP（自动转 /32 或 /128） | `203.0.113.50` |
| 精确域名 | `{ domain = "example.com" }` | DNS 解析后匹配 | `gitlab.corp.com` |
| 通配符域名 | `{ domain_pattern = "*.example.com" }` | DNS 解析后匹配 | `*.corp.example.com` |

### 4.7 路由表 ID 分配

守护进程使用 Linux 策略路由，自动分配路由表 ID：

| 范围 | 用途 |
|------|------|
| `table_id_start` ~ `table_id_start + 99` | IPv4 路由规则 |
| 200 ~ 299 | IPv6 路由规则 |

### 4.8 配置热加载

守护进程自动监听配置文件变化。修改配置文件保存后，无需重启即可生效：

```bash
# 编辑配置
sudo vim /etc/nic-autoswitch/config.toml

# 保存后自动生效，也可手动触发重载
sudo systemctl reload nic-autoswitch
# 或
nic-autoswitch-cli reload
```

### 4.9 配置验证

配置必须满足以下条件，否则加载失败：

- 至少定义一个网络接口
- `table_id_start` 必须在 100-199 范围内
- 规则名不能为空
- `route_via.interface` 不能为空
- 每条规则的 `match_on` 必须指定有效的匹配条件

---

## 5. 运行

### 5.1 前台运行（调试用）

```bash
# 使用默认配置文件
sudo nic-autoswitch -f

# 指定配置和日志级别
sudo nic-autoswitch -f -c /path/to/config.toml -l debug

# 干运行模式（不修改路由表）
sudo nic-autoswitch -f --dry-run
```

**命令行参数**：

| 参数 | 缩写 | 默认值 | 说明 |
|------|------|--------|------|
| `--config` | `-c` | `/etc/nic-autoswitch/config.toml` | 配置文件路径 |
| `--dry-run` | | false | 干运行模式 |
| `--log-level` | `-l` | info | 日志级别 |
| `--foreground` | `-f` | false | 前台运行（不创建子进程） |

### 5.2 systemd 服务

```bash
# 启动服务
sudo systemctl start nic-autoswitch

# 查看状态
sudo systemctl status nic-autoswitch

# 停止服务
sudo systemctl stop nic-autoswitch

# 重载配置（发送 SIGHUP）
sudo systemctl reload nic-autoswitch

# 开机自启
sudo systemctl enable nic-autoswitch

# 查看日志
journalctl -u nic-autoswitch -f
```

### 5.3 信号处理

守护进程响应以下信号：

| 信号 | 作用 |
|------|------|
| `SIGTERM` / `SIGINT` | 优雅关闭 |
| `SIGHUP` | 重载配置文件 |

---

## 6. CLI 工具

`nic-autoswitch-cli` 通过 Unix socket (`/run/nic-autoswitch/control.sock`) 与守护进程通信。

### 6.1 命令

```bash
# 查看守护进程状态
nic-autoswitch-cli status

# 查看活跃路由规则
nic-autoswitch-cli routes

# 手动重载配置
nic-autoswitch-cli reload

# 检查守护进程是否运行
nic-autoswitch-cli ping

# 请求守护进程关闭
nic-autoswitch-cli shutdown
```

### 6.2 选项

```bash
# 指定控制 socket 路径
nic-autoswitch-cli -s /run/nic-autoswitch/control.sock status
```

| 选项 | 缩写 | 默认值 | 说明 |
|------|------|--------|------|
| `--socket` | `-s` | `/run/nic-autoswitch/control.sock` | 控制socket路径 |

---

## 7. Docker 开发环境

项目提供 Docker 开发环境，用于在隔离环境中开发和测试（模拟多网卡环境）。

### 7.1 构建镜像

```bash
docker compose build
```

### 7.2 交互式开发

```bash
# 进入容器（自动创建虚拟网络接口 nic0/nic1/nic2）
docker compose run --rm dev

# 容器内可用命令：
cargo build                           # 编译
cargo test                            # 运行全部测试
cargo run -- --dry-run -f             # 干运行模式
ip route show                         # 查看路由表
ip rule show                          # 查看策略路由规则
ip -br addr                           # 查看接口地址
```

容器内模拟的网络接口：

| 接口 | 类型 | IP 地址 | 用途 |
|------|------|---------|------|
| `nic0` | LAN | 192.168.1.100/24 | 有线网络 |
| `nic1` | WLAN | 192.168.2.100/24 | 无线网络 |
| `nic2` | VPN | 10.8.0.1/24 | VPN 隧道 |

### 7.3 运行测试

```bash
# 运行全部测试（含单元+集成）
docker compose run --rm test

# 仅运行集成测试
docker compose run --rm test-integration

# 运行特定测试文件
docker compose run --rm dev
# 然后容器内执行：
cargo test --test integration_config -- --test-threads=1
```

---

## 8. 测试

### 8.1 测试结构

```
tests/
├── common/                         # 共享测试工具
│   ├── mod.rs                      # 工具函数、配置构建器
│   └── mock_network.rs             # Mock 网络组件
├── fixtures/                       # 测试配置文件
│   ├── valid_config.toml
│   ├── complex_config.toml
│   ├── ipv6_config.toml
│   └── invalid/                    # 错误配置
├── integration_config.rs           # 配置加载测试
├── integration_engine.rs           # 引擎调度测试
├── integration_route.rs            # 路由管理测试
├── integration_daemon.rs           # 守护进程生命周期测试
└── integration_cli.rs              # CLI 通信测试
```

### 8.2 运行测试

```bash
# 单元测试（无需特权）
cargo test --lib

# 集成测试（无需特权）
cargo test --test integration_config --test integration_engine

# 路由/daemon/CLI 集成测试（需要 NET_ADMIN，在 Docker 中运行）
docker compose run --rm test

# 运行所有测试
cargo test --all-features

# 运行单个测试
cargo test test_load_valid_config

# 运行基准测试
cargo bench
```

### 8.3 测试覆盖率

```bash
cargo tarpaulin --out Html --output-dir coverage/
```

目标覆盖率：行覆盖率 ≥ 80%，关键路径 100%。

---

## 9. 开发工作流

### 9.1 环境准备

```bash
# 安装 Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# 安装 pre-commit hooks
pre-commit install
```

### 9.2 开发检查

```bash
# 格式化 + 静态检查 + 测试
cargo fmt && cargo clippy -- -D warnings && cargo test
```

### 9.3 Git 分支规范

- 分支命名：`feature/*`, `fix/*`, `docs/*`
- 提交格式：Conventional Commits

```
feat(router): add IPv6 support
fix(config): validate table_id_start range
docs(readme): update configuration examples
test(engine): add dispatcher lifecycle tests
```

### 9.4 CI 流水线

项目通过 GitHub Actions 自动运行：

1. **Lint**：`cargo fmt --check` + `cargo clippy`
2. **Test**：`cargo test --all-features`
3. **Coverage**：`cargo tarpaulin` + Codecov 上传
4. **Security**：`cargo audit`
5. **Build**：`cargo build --release`
6. **Integration Tests (Docker)**：在容器中运行集成测试

---

## 10. 故障排除

### 10.1 查看日志

```bash
# 实时日志
journalctl -u nic-autoswitch -f

# 最近 100 行
journalctl -u nic-autoswitch -n 100

# 调试级别
sudo nic-autoswitch -f -l debug
```

### 10.2 常见问题

#### 权限不足

```
Error: Permission denied (os error 13)
```

**解决**：使用 `sudo` 运行或授予 `CAP_NET_ADMIN`：

```bash
sudo setcap cap_net_admin+ep /usr/bin/nic-autoswitch
```

#### 配置文件不存在

```
Error: Config file not found: /etc/nic-autoswitch/config.toml
```

**解决**：

```bash
sudo mkdir -p /etc/nic-autoswitch
sudo cp config.example.toml /etc/nic-autoswitch/config.toml
```

#### 配置验证失败

```
Error: At least one interface must be configured
```

**解决**：在配置文件中至少定义一个 `[interfaces.*]` 段。

#### 路由表 ID 无效

```
Error: table_id_start must be between 100 and 199
```

**解决**：调整 `[global]` 中的 `table_id_start` 值到有效范围。

#### NetworkManager 不可用

守护进程启动时会检测 NetworkManager 可用性。如果 NM 未运行，SSID 监控功能降级为仅 netlink 模式。这不影响基于 CIDR/IP 的路由规则，仅影响 WiFi 配置文件的自动切换。

#### DNS 解析失败

基于域名的规则需要 DNS 解析。确保系统 DNS 配置正确：

```bash
# 检查 DNS 配置
cat /etc/resolv.conf

# 手动测试解析
nslookup gitlab.corp.com
```

### 10.3 调试路由

```bash
# 查看所有路由表
ip route show table all

# 查看特定路由表
ip route show table 100

# 查看策略路由规则
ip rule show

# 查看 IPv6 路由
ip -6 route show table 200
```

### 10.4 手动验证配置

```bash
# 干运行模式查看将要执行的操作
sudo nic-autoswitch -f --dry-run -l debug
```
