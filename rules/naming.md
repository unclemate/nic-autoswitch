# 命名规范

本规范遵循 Rust 官方命名约定（RFC 430），确保代码风格一致性。

## 基本规则

| 类型 | 规范 | 示例 |
|------|------|------|
| 模块名 (mod) | snake_case | `network_manager`, `route_table` |
| 类型/结构体/枚举 | PascalCase | `NetworkEvent`, `RouteRule`, `ConfigLoader` |
| 函数/方法 | snake_case | `apply_route_rules()`, `load_config()` |
| 变量/参数 | snake_case | `interface_name`, `table_id`, `ssid` |
| 常量 | SCREAMING_SNAKE_CASE | `DEFAULT_TABLE_ID`, `MAX_RETRY_COUNT` |
| 静态变量 | SCREAMING_SNAKE_CASE | `GLOBAL_CONFIG`, `VERSION` |
| 生命周期参数 | 短小写字母 | `'a`, `'ctx`, `'cfg` |
| 泛型参数 | PascalCase（短） | `T`, `E`, `Ctx`, `R` |
| 宏 | snake_case! | `define_event!`, `impl_from_error!` |
| 文件名 | snake_case.rs | `network_manager.rs`, `config_loader.rs` |

## 项目特定命名

### 模块命名

```
src/
├── config/           # 配置相关
│   ├── mod.rs
│   ├── schema.rs     # 配置数据结构
│   └── loader.rs     # 配置加载器
├── monitor/          # 网络监控
│   ├── mod.rs
│   ├── netlink.rs    # netlink 监控
│   ├── networkmanager.rs  # D-Bus 监控
│   ├── state.rs      # 网络状态
│   └── events.rs     # 事件定义
├── router/           # 路由管理
│   ├── mod.rs
│   ├── manager.rs    # 路由表管理
│   ├── dns.rs        # DNS 解析
│   └── rules.rs      # 规则操作
├── engine/           # 核心引擎
│   ├── mod.rs
│   ├── dispatcher.rs # 事件分发
│   ├── matcher.rs    # 规则匹配
│   └── executor.rs   # 规则执行
├── daemon/           # 守护进程
│   ├── mod.rs
│   ├── service.rs    # 主服务
│   ├── signals.rs    # 信号处理
│   ├── systemd.rs    # systemd 集成
│   └── control.rs    # Unix socket 服务端
├── cli/              # CLI 工具
│   ├── mod.rs
│   ├── client.rs     # Unix socket 客户端
│   └── commands.rs   # 命令实现
└── error.rs          # 错误类型
```

### 类型命名

```rust
// 结构体：描述性名词
pub struct NetworkState { }
pub struct RouteRule { }
pub struct Config { }
pub struct WifiProfile { }

// 枚举：描述性名词，变体用 PascalCase
pub enum NetworkEvent {
    InterfaceChanged { interface: String, change: InterfaceChange },
    WifiConnected { interface: String, ssid: String },
    WifiDisconnected { interface: String, last_ssid: Option<String> },
    AddressChanged { interface: String, added: Vec<IpNetwork>, removed: Vec<IpNetwork> },
}

pub enum InterfaceType {
    Lan,
    Wlan,
    Vpn,
}

// trait：描述能力的形容词或名词
pub trait RouteProvider { }
pub trait EventDispatcher { }
pub trait ConfigValidator { }
```

### 函数命名

```rust
// 动词开头，描述行为
pub fn load_config() { }
pub fn apply_route_rules() { }
pub fn match_destination() { }
pub fn resolve_dns() { }

// 布尔返回值用 is_/has_/can_ 前缀
pub fn is_interface_up(&self) -> bool { }
pub fn has_ipv6_address(&self) -> bool { }
pub fn can_resolve(&self) -> bool { }

// getter 不加 get_ 前缀
pub fn interface_name(&self) -> &str { }
pub fn table_id(&self) -> u32 { }

// setter 用 set_ 前缀
pub fn set_log_level(&mut self, level: LogLevel) { }
```

### 变量命名

```rust
// 使用有意义的名称
let interface_name = "eth0";
let table_id = 100;
let current_ssid = wifi_state.ssid;

// 避免单字母变量（除非循环索引或短作用域）
// 好
for rule in rules {
    process_rule(rule);
}

// 避免
let r = rules.first();

// 布尔变量用 is_/has_/should_ 前缀
let is_connected = state.is_connected();
let has_default_route = routes.iter().any(|r| r.is_default());
```

### 常量命名

```rust
// 模块级常量
pub const DEFAULT_TABLE_ID: u32 = 100;
pub const MAX_RETRY_COUNT: u32 = 3;
pub const MONITOR_INTERVAL_SECS: u64 = 5;
pub const SOCKET_PATH: &str = "/run/nic-autoswitch/ctl.sock";

// 关联常量
impl Config {
    pub const DEFAULT_LOG_LEVEL: &str = "info";
    pub const DEFAULT_DRY_RUN: bool = false;
}
```

## 缩写规则

- 保留通用缩写：`IP`, `DNS`, `TCP`, `UDP`, `SSID`, `API`
- 缩写在类型名中保持大写首字母：`IpAddress`, `DnsResolver`, `TcpConnection`
- 缩写在变量/函数中保持小写：`ip_address`, `dns_resolver`, `tcp_connection`

```rust
// 好
pub struct IpAddress { }
pub fn resolve_dns() { }
let ssid = network.ssid();

// 避免
pub struct IPAddress { }  // 不一致的缩写风格
pub fn resolveDNS() { }   // 混合大小写
```

## 命名冲突处理

```rust
// 使用别名解决冲突
use std::io::Error as IoError;
use crate::error::Error as AppError;

// 或明确使用完整路径
fn handle_error(e: std::io::Error) { }
```
