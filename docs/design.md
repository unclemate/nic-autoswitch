# nic-autoswitch 实现计划

## 背景

nic-autoswitch 是一个Linux网络自动分流守护进程，用于根据网络接口状态自动管理流量路由。主要解决以下问题：

- **多网卡环境下的智能路由**：笔记本同时连接有线(LAN)和无线(WLAN)时，需要根据目标地址选择最优路径
- **企业网络访问优化**：公司内部服务走内网，外部流量走公网，提升访问效率和安全性
- **网络切换自动化**：WiFi/有线连接变化时自动调整路由规则，无需手动干预

## 技术栈

| 组件 | 技术选择 | 理由 |
|------|----------|------|
| 运行时 | tokio (async) | 高性能异步，适合事件驱动 |
| 日志 | tracing + tracing-journald | systemd原生集成 |
| 配置 | config-rs + serde + toml | Rust生态标准，人类可读 |
| 信号处理 | signal-hook + signal-hook-tokio | 现代信号处理 |
| 网络监控 | rtnetlink + zbus | 内核直连 + NetworkManager D-Bus |
| DNS解析 | hickory-resolver | 异步DNS，支持缓存 |
| 错误处理 | anyhow + thiserror | 灵活的错误处理 |

## 项目结构

```
nic-autoswitch/
├── Cargo.toml                    # workspace
├── config.example.toml
├── src/
│   ├── main.rs                   # 入口
│   ├── lib.rs
│   ├── config/                   # 配置管理
│   │   ├── mod.rs
│   │   ├── schema.rs             # 配置数据结构
│   │   └── loader.rs             # 配置加载+热加载
│   ├── monitor/                  # 网络监控
│   │   ├── mod.rs
│   │   ├── netlink.rs            # rtnetlink监控(IPv4+IPv6)
│   │   ├── networkmanager.rs     # D-Bus监控SSID
│   │   ├── state.rs              # 网络状态
│   │   └── events.rs             # 事件定义
│   ├── router/                   # 路由管理
│   │   ├── mod.rs
│   │   ├── manager.rs            # 路由表管理(IPv4+IPv6)
│   │   ├── dns.rs                # DNS解析
│   │   └── rules.rs
│   ├── engine/                   # 核心引擎
│   │   ├── mod.rs
│   │   ├── dispatcher.rs         # 事件分发
│   │   ├── matcher.rs            # 规则匹配
│   │   └── executor.rs           # 规则执行
│   ├── daemon/                   # 守护进程
│   │   ├── mod.rs
│   │   ├── service.rs            # 主服务
│   │   ├── signals.rs            # 信号处理
│   │   ├── systemd.rs            # systemd集成
│   │   └── control.rs            # Unix socket服务端
│   ├── cli/                      # CLI工具
│   │   ├── mod.rs
│   │   ├── client.rs             # Unix socket客户端
│   │   └── commands.rs           # 命令实现
│   └── error.rs
├── systemd/
│   └── nic-autoswitch.service
└── tests/
```

## 核心数据结构

### 配置格式 (config.toml)

```toml
[global]
monitor_interval = 5
log_level = "info"
dry_run = false
table_id_start = 100

[interfaces.eth0]
interface_type = "lan"
match_by = { name = "eth0" }
priority = 10

[interfaces.wlan0]
interface_type = "wlan"
match_by = { name = "wlan0" }
priority = 20

# WiFi SSID特定规则
[wifi_profiles."CorpWiFi"]
interface = "wlan0"

[[wifi_profiles."CorpWiFi".rules]]
name = "corp-cidr"
match_on = { cidr = "10.0.0.0/8" }      # IP网段
route_via = { interface = "eth0" }
priority = 100

[[wifi_profiles."CorpWiFi".rules]]
name = "corp-single-ip"
match_on = { ip = "203.0.113.50" }      # 单个IP地址
route_via = { interface = "eth0" }
priority = 110

[[wifi_profiles."CorpWiFi".rules]]
name = "corp-exact-domain"
match_on = { domain = "gitlab.corp.com" }  # 精确域名
route_via = { interface = "eth0" }
priority = 120

[[wifi_profiles."CorpWiFi".rules]]
name = "corp-wildcard-domain"
match_on = { domain_pattern = "*.corp.example.com" }  # 通配符域名
route_via = { interface = "eth0" }
priority = 130

# 默认规则
[[routing.default_rules]]
name = "default-wlan"
match_on = { cidr = "0.0.0.0/0" }
route_via = { interface = "wlan0" }
priority = 10000
```

### 分流规则匹配条件

| 条件类型 | 配置语法 | 说明 | 示例 |
|----------|----------|------|------|
| IP网段 | `match_on = { cidr = "x.x.x.x/y" }` | CIDR格式网段 | `10.0.0.0/8`, `192.168.1.0/24` |
| 单个IP | `match_on = { ip = "x.x.x.x" }` | 单个IPv4/IPv6地址 | `203.0.113.50`, `2001:db8::1` |
| 精确域名 | `match_on = { domain = "example.com" }` | 完整域名匹配 | `gitlab.corp.com` |
| 通配符域名 | `match_on = { domain_pattern = "*.example.com" }` | 支持前缀通配符 | `*.corp.example.com` |

### 匹配逻辑

```
┌─────────────────────────────────────────────────────────────┐
│                    Rule Matching Flow                       │
├─────────────────────────────────────────────────────────────┤
│                                                             │
│  输入: 目标地址 (IP 或 域名)                                  │
│                                                             │
│  1. 如果是IP地址:                                            │
│     ├─→ 检查 ip 精确匹配                                     │
│     └─→ 检查 cidr 范围匹配                                   │
│                                                             │
│  2. 如果是域名:                                              │
│     ├─→ 检查 domain 精确匹配                                 │
│     ├─→ 检查 domain_pattern 通配符匹配                       │
│     └─→ DNS解析为IP → 走IP匹配逻辑                           │
│                                                             │
│  3. 返回匹配的规则列表 (按priority排序)                       │
│                                                             │
└─────────────────────────────────────────────────────────────┘
```

### 网络事件

```rust
enum NetworkEvent {
    InterfaceChanged { interface: String, change: InterfaceChange },
    WifiConnected { interface: String, ssid: String },
    WifiDisconnected { interface: String, last_ssid: Option<String> },
    AddressChanged { interface: String, added: Vec<IpNetwork>, removed: Vec<IpNetwork> },
    RouteChanged { interface: String, ... },
}
```

## 核心流程

### 1. 启动流程

1. 解析命令行参数
2. 初始化日志 (tracing + journald)
3. 加载配置文件
4. 初始化组件 (Netlink, D-Bus, Router)
5. 扫描当前网络状态
6. 应用初始路由规则
7. 通知 systemd 就绪
8. 进入事件循环

### 2. 事件处理流程

```
Netlink/D-Bus事件 → 事件规范化 → 更新NetworkState
                                    ↓
                              规则匹配求值
                                    ↓
                              计算路由变更差异
                                    ↓
                              执行路由操作
                                    ↓
                              记录日志
```

### 3. 路由更新策略

使用 Linux 策略路由：
- 为每个接口分配独立路由表 (table_id: 100, 101, 102...)
- 使用 `ip rule` 根据目标地址/源地址选择路由表
- 域名规则通过DNS解析后动态添加

## 扩展应用场景

除了用户提到的基本场景外，本工具还可支持：

1. **VPN智能路由**
   - VPN连接后，仅特定流量（如公司内网）走VPN隧道
   - 其他流量保持直连，减少延迟

2. **流量优先级管理**
   - 关键业务流量优先走低延迟接口
   - 大流量下载走备用接口

3. **故障自动切换**
   - 主接口断开时自动将流量切换到备用接口
   - 恢复后自动切回

4. **按时间段路由**
   - 工作时间走公司网络
   - 非工作时间走家庭网络

5. **多ISP负载分流**
   - 不同类型流量走不同ISP（如视频走带宽大的，游戏走延迟低的）

6. **开发者测试环境**
   - 测试环境域名自动路由到测试网络
   - 生产域名走正常路径

7. **安全隔离**
   - 敏感服务仅允许通过特定接口访问
   - 防止流量泄漏到错误网络

## 实现阶段

### Phase 1: 基础框架 (预计文件: 5)
- [ ] `src/error.rs` - 错误类型定义
- [ ] `src/config/schema.rs` - 配置数据结构（含IPv4/IPv6支持）
- [ ] `src/config/loader.rs` - 配置加载验证
- [ ] `src/daemon/signals.rs` - 信号处理
- [ ] `src/main.rs` - 基本启动逻辑

### Phase 2: 监控层 (预计文件: 5)
- [ ] `src/monitor/events.rs` - 网络事件定义
- [ ] `src/monitor/state.rs` - 网络状态管理
- [ ] `src/monitor/netlink.rs` - rtnetlink监控（IPv4+IPv6路由组）
- [ ] `src/monitor/networkmanager.rs` - D-Bus监控

### Phase 3: 路由层 (预计文件: 4)
- [ ] `src/router/dns.rs` - DNS解析（AAAA记录支持）
- [ ] `src/router/manager.rs` - 路由表管理（双栈支持）
- [ ] `src/router/rules.rs` - 规则操作

### Phase 4: 引擎层 (预计文件: 5)
- [ ] `src/engine/matcher.rs` - 规则匹配
- [ ] `src/engine/executor.rs` - 规则执行
- [ ] `src/engine/dispatcher.rs` - 事件分发

### Phase 5: 集成 (预计文件: 5)
- [ ] `src/daemon/service.rs` - 主服务协调器
- [ ] `src/daemon/systemd.rs` - systemd集成
- [ ] `src/daemon/control.rs` - Unix socket服务端
- [ ] `src/config/watcher.rs` - 配置热加载监听
- [ ] `systemd/nic-autoswitch.service` - 服务文件

### Phase 6: CLI工具 (预计文件: 3)
- [ ] `src/cli/main.rs` - CLI入口
- [ ] `src/cli/client.rs` - Unix socket客户端
- [ ] `src/cli/commands.rs` - 命令实现

### Phase 7: 测试和文档
- [ ] 单元测试
- [ ] 集成测试
- [ ] 示例配置文件
- [ ] CLI帮助文档

## 依赖清单 (Cargo.toml)

```toml
[package]
name = "nic-autoswitch"
version = "0.1.0"
edition = "2024"

[[bin]]
name = "nic-autoswitch"
path = "src/main.rs"

[[bin]]
name = "nic-autoswitch-cli"
path = "src/cli/main.rs"

[dependencies]
tokio = { version = "1", features = ["rt-multi-thread", "macros", "signal", "time", "sync", "net"] }
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter", "fmt"] }
tracing-journald = "0.3"
config = "0.14"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
toml = "0.8"
signal-hook = "0.3"
signal-hook-tokio = { version = "0.3", features = ["futures-v0_3"] }
rtnetlink = "0.14"
netlink-packet-route = "0.19"
zbus = { version = "5", default-features = false, features = ["tokio"] }
hickory-resolver = "0.24"
ipnetwork = "0.20"
anyhow = "1"
thiserror = "2"
notify = "6"              # 配置文件热加载监听
clap = { version = "4", features = ["derive"] }  # CLI参数解析

[dev-dependencies]
tokio-test = "0.4"
tempfile = "3"
```

## 验证方法

1. **编译检查**: `cargo build`
2. **单元测试**: `cargo test`
3. **手动测试**:
   - 创建虚拟网络接口: `ip link add veth_test type veth peer name veth_test_peer`
   - 运行程序: `sudo ./target/debug/nic-autoswitch --config config.example.toml`
   - 验证路由表: `ip route show table 100`
   - 验证规则: `ip rule show`
4. **日志检查**: `journalctl -u nic-autoswitch -f`

## 用户确认的设计决策

| 决策项 | 选择 | 影响 |
|--------|------|------|
| IPv6支持 | 同时支持IPv4和IPv6 | 需要处理两套路由表和规则，netlink订阅RTNLGRP_IPV6_ROUTE |
| 连接保持 | 不需要保持 | 简化实现，路由变更后新连接生效 |
| CLI工具 | 提供nic-autoswitch-cli | 需要添加CLI子模块，支持status/refresh/show等命令 |
| 配置热加载 | 自动热加载 | 使用notify库监听文件变化，触发重新加载 |

## CLI工具设计 (nic-autoswitch-cli)

```
nic-autoswitch-cli <command> [options]

Commands:
  status          显示当前网络状态和活跃路由规则
  refresh         手动触发路由规则重新计算和应用
  show routes     显示当前系统的路由表
  show rules      显示当前系统的策略路由规则
  show dns-cache  显示DNS缓存内容
  config check    验证配置文件语法
  config reload   触发配置重新加载

Options:
  --json          JSON格式输出
  --verbose       详细输出
```

实现方式：
- CLI通过Unix socket与daemon通信
- 通信协议使用简单的文本协议或JSON-RPC
- Daemon监听 `/run/nic-autoswitch/ctl.sock`

## 注意事项

1. **需要root权限**: 操作路由表需要 CAP_NET_ADMIN 能力
2. **依赖NetworkManager**: SSID监控依赖D-Bus和NetworkManager，无NM时降级为仅netlink模式
3. **IPv6双栈**: 路由表ID范围需要为IPv4和IPv6分别规划（如100-199 IPv4, 200-299 IPv6）
