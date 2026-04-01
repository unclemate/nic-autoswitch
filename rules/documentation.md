# 文档注释规范

本文档定义 nic-autoswitch 项目的 rustdoc 文档注释标准。

## 基本格式

### 公共 API 必须有文档注释

```rust
/// 简短的一行描述
///
/// 更详细的描述（可选），可以多行。
/// 解释函数的行为、用途和注意事项。
///
/// # Arguments
///
/// * `param_name` - 参数说明
///
/// # Returns
///
/// 返回值说明
///
/// # Errors
///
/// 描述可能的错误情况
///
/// # Example
///
/// ```rust
/// use nic_autoswitch::router::Manager;
///
/// let manager = Manager::new();
/// manager.apply_rule(&rule)?;
/// ```
///
/// # Panics
///
/// 描述可能导致 panic 的情况（如果有）
pub fn apply_rule(&self, rule: &RouteRule) -> Result<()> {
    // ...
}
```

## 各部分说明

### 简短描述

- 第一行是简短摘要（一行）
- 以动词开头（第三人称单数）
- 不超过 80 个字符
- 不以句号结尾

```rust
/// 应用指定的路由规则到系统路由表
pub fn apply_rule(&self, rule: &RouteRule) -> Result<()> { }

/// 解析域名到 IP 地址列表
pub async fn resolve(&self, domain: &str) -> Result<Vec<IpAddr>> { }
```

### Arguments（参数）

- 列出每个参数的用途
- 使用 `* \`name\` - description` 格式
- 如果参数用途明显，可省略

```rust
///
/// # Arguments
///
/// * `config_path` - 配置文件的路径
/// * `validate` - 是否执行配置验证
pub fn load(config_path: &Path, validate: bool) -> Result<Config> { }
```

### Returns（返回值）

- 描述返回值含义
- 如果返回 `Self` 或明显类型，可省略

```rust
///
/// # Returns
///
/// 匹配到的路由规则，如果没有匹配则返回 `None`
pub fn match_destination(&self, dest: &Destination) -> Option<&RouteRule> { }
```

### Errors（错误）

- 描述返回 `Err` 的所有情况
- 对于 `Result<T, E>` 类型必须包含

```rust
///
/// # Errors
///
/// 返回错误的情况：
/// - `Error::ConfigNotFound` - 配置文件不存在
/// - `Error::ConfigInvalid` - 配置格式无效
/// - `Error::Io` - 读取文件时发生 I/O 错误
pub fn load(path: &Path) -> Result<Config> { }
```

### Panics（恐慌）

- 仅当函数可能 panic 时需要
- 描述导致 panic 的条件

```rust
///
/// # Panics
///
/// 如果 `index` 超出范围会 panic。使用 `get` 方法进行安全访问。
pub fn remove_rule(&mut self, index: usize) { }
```

### Example（示例）

- 提供可运行的代码示例
- 使用 ` ```rust ` 代码块标记
- 示例应该是完整且可编译的

```rust
///
/// # Example
///
/// ```rust
/// use nic_autoswitch::config::{Config, ConfigLoader};
/// use std::path::Path;
///
/// let config = ConfigLoader::load(Path::new("config.toml"))?;
/// println!("Loaded {} interfaces", config.interfaces.len());
/// # Ok::<(), nic_autoswitch::Error>(())
/// ```
pub fn load(path: &Path) -> Result<Config> { }
```

### Safety（安全性）

- 对于 `unsafe` 函数必须包含
- 解释安全使用的条件

```rust
/// # Safety
///
/// 调用者必须确保：
/// - `ptr` 指向有效的内存
/// - 内存在函数调用期间不会被释放
pub unsafe fn read_raw(ptr: *const u8) -> u8 { }
```

## 结构体和枚举文档

### 结构体

```rust
/// 网络接口配置
///
/// 定义单个网络接口的路由规则和优先级。
///
/// # Example
///
/// ```rust
/// use nic_autoswitch::config::InterfaceConfig;
///
/// let eth0 = InterfaceConfig {
///     interface_type: InterfaceType::Lan,
///     match_by: MatchBy::Name("eth0".to_string()),
///     priority: 10,
/// };
/// ```
#[derive(Debug, Clone, Deserialize)]
pub struct InterfaceConfig {
    /// 接口类型（lan, wlan, vpn）
    pub interface_type: InterfaceType,

    /// 接口匹配条件
    pub match_by: MatchBy,

    /// 优先级（数字越小优先级越高）
    pub priority: u32,
}
```

### 枚举

```rust
/// 网络事件类型
///
/// 表示监控系统可能产生的所有网络事件。
#[derive(Debug, Clone)]
pub enum NetworkEvent {
    /// 接口状态变化
    ///
    /// 字段：
    /// - `interface` - 接口名称
    /// - `change` - 变化类型
    InterfaceChanged {
        interface: String,
        change: InterfaceChange,
    },

    /// WiFi 连接事件
    WifiConnected {
        /// 接口名称
        interface: String,
        /// 连接的 SSID
        ssid: String,
    },

    /// WiFi 断开事件
    WifiDisconnected {
        interface: String,
        /// 断开前连接的 SSID（如果有）
        last_ssid: Option<String>,
    },
}
```

## Trait 文档

```rust
/// 网络监控器 trait
///
/// 定义网络状态监控的通用接口，允许不同的实现
///（如 Netlink、Mock 等）互换使用。
///
/// # Implementors
///
/// 实现此 trait 时需确保：
/// - `current_state()` 返回当前的网络状态快照
/// - `events()` 返回的事件流永不结束，除非发生错误
///
/// # Example
///
/// ```rust
/// use nic_autoswitch::monitor::NetworkMonitor;
///
/// async fn watch_network(monitor: &dyn NetworkMonitor) {
///     while let Some(event) = monitor.events().next().await {
///         println!("Network event: {:?}", event);
///     }
/// }
/// ```
pub trait NetworkMonitor: Send + Sync {
    /// 获取当前网络状态
    fn current_state(&self) -> NetworkState;

    /// 获取网络事件流
    fn events(&self) -> BoxStream<'_, NetworkEvent>;
}
```

## 模块文档

```rust
//! 路由管理模块
//!
//! 提供路由表的创建、修改和查询功能。
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────┐     ┌─────────────┐
//! │   Manager   │────▶│   Rules     │
//! └─────────────┘     └─────────────┘
//!        │                   │
//!        ▼                   ▼
//! ┌─────────────┐     ┌─────────────┐
//! │    DNS      │     │   Tables    │
//! └─────────────┘     └─────────────┘
//! ```
//!
//! # Example
//!
//! ```rust
//! use nic_autoswitch::router::Manager;
//!
//! let manager = Manager::new()?;
//! manager.apply_default_routes()?;
//! ```

pub mod manager;
pub mod dns;
pub mod rules;
```

## 文档测试

### 跳过文档测试

```rust
/// ```no_run
/// // 编译但不运行
/// let manager = Manager::new()?;
/// manager.run().await?;  // 阻塞调用
/// ```

/// ```ignore
/// // 不编译也不运行
/// let x = some_undefined_thing;
/// ```

/// ```compile_fail
/// // 期望编译失败
/// let x: i32 = "string";  // 类型错误
/// ```
```

### 隐藏示例中的辅助代码

```rust
/// # use nic_autoswitch::router::Manager;
/// # fn main() -> Result<(), Error> {
/// let manager = Manager::new()?;
/// manager.apply_rule(&rule)?;
/// # Ok(())
/// # }
```

## 文档生成

```bash
# 生成文档
cargo doc

# 生成并打开文档
cargo doc --open

# 包含私有项
cargo doc --document-private-items
```

## 文档检查

在 CI 中验证文档：

```bash
# 检查文档警告
RUSTDOCFLAGS="-D warnings" cargo doc
```
