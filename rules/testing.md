# 测试规范

本文档定义 nic-autoswitch 项目的测试策略、组织结构和覆盖率要求。

## 测试组织结构

```
nic-autoswitch/
├── src/
│   ├── config/
│   │   ├── mod.rs              // #[cfg(test)] mod tests { ... }
│   │   ├── schema.rs           // 内联单元测试
│   │   └── loader.rs           // 内联单元测试
│   ├── router/
│   │   ├── manager.rs          // 内联单元测试
│   │   ├── dns.rs              // #[tokio::test] 异步测试
│   │   └── rules.rs
│   └── engine/
│       └── matcher.rs          // 内联单元测试
├── tests/                       // 集成测试目录
│   ├── integration/
│   │   ├── config_reload_test.rs
│   │   ├── route_management_test.rs
│   │   ├── daemon_lifecycle_test.rs
│   │   └── cli_commands_test.rs
│   ├── fixtures/
│   │   ├── valid_config.toml
│   │   ├── invalid_config.toml
│   │   └── complex_config.toml
│   └── common/
│       ├── mod.rs              // 共享测试工具
│       └── mock_network.rs     // mock 网络组件
└── benches/                     // 性能基准测试
    └── matcher_bench.rs
```

## 单元测试

### 测试命名规范

```rust
// 格式: test_<函数名>_<场景>_<期望结果>
#[test]
fn test_match_rule_with_cidr_returns_true() { }

#[test]
fn test_match_rule_with_invalid_cidr_returns_false() { }

#[test]
fn test_load_config_missing_file_returns_error() { }

#[test]
fn test_apply_route_with_valid_rule_succeeds() { }
```

### 内联测试示例

```rust
// src/engine/matcher.rs

pub struct RuleMatcher {
    rules: Vec<RouteRule>,
}

impl RuleMatcher {
    pub fn match_destination(&self, dest: &Destination) -> Option<&RouteRule> {
        self.rules.iter()
            .find(|r| r.matches(dest))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_matcher() -> RuleMatcher {
        RuleMatcher {
            rules: vec![
                RouteRule {
                    name: "corp-cidr".to_string(),
                    match_on: MatchOn::Cidr("10.0.0.0/8".parse().unwrap()),
                    route_via: "eth0".to_string(),
                    priority: 100,
                },
            ],
        }
    }

    #[test]
    fn test_match_destination_with_matching_cidr_returns_rule() {
        let matcher = create_test_matcher();
        let dest = Destination::Ip("10.1.2.3".parse().unwrap());

        let result = matcher.match_destination(&dest);

        assert!(result.is_some());
        assert_eq!(result.unwrap().name, "corp-cidr");
    }

    #[test]
    fn test_match_destination_with_non_matching_cidr_returns_none() {
        let matcher = create_test_matcher();
        let dest = Destination::Ip("8.8.8.8".parse().unwrap());

        let result = matcher.match_destination(&dest);

        assert!(result.is_none());
    }
}
```

### 异步测试

```rust
// src/router/dns.rs

pub struct DnsResolver {
    cache: Arc<Mutex<HashMap<String, Vec<IpAddr>>>>,
}

impl DnsResolver {
    pub async fn resolve(&self, domain: &str) -> Result<Vec<IpAddr>> {
        // 检查缓存
        if let Some(cached) = self.cache.lock().await.get(domain) {
            return Ok(cached.clone());
        }

        // DNS 解析逻辑...
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_resolve_domain_caches_result() {
        let resolver = DnsResolver::new();

        let first = resolver.resolve("example.com").await.unwrap();
        let second = resolver.resolve("example.com").await.unwrap();

        // 两次结果应相同
        assert_eq!(first, second);
    }

    #[tokio::test]
    async fn test_resolve_invalid_domain_returns_error() {
        let resolver = DnsResolver::new();

        let result = resolver.resolve("invalid.invalid").await;

        assert!(result.is_err());
    }
}
```

## 集成测试

### 测试场景

| 场景 | 文件 | 描述 |
|------|------|------|
| 配置加载 | `config_reload_test.rs` | 验证配置加载、验证、热重载 |
| 路由管理 | `route_management_test.rs` | 验证路由规则应用、删除 |
| 守护进程生命周期 | `daemon_lifecycle_test.rs` | 启动、停止、信号处理 |
| CLI 命令 | `cli_commands_test.rs` | status/refresh/show 命令 |

### 集成测试示例

```rust
// tests/integration/config_reload_test.rs

use std::fs;
use tempfile::TempDir;
use nic_autoswitch::config::{Config, ConfigLoader};

fn create_temp_config(content: &str) -> (TempDir, std::path::PathBuf) {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("config.toml");
    fs::write(&path, content).unwrap();
    (dir, path)
}

#[test]
fn test_load_valid_config_succeeds() {
    let content = r#"
[global]
monitor_interval = 5
log_level = "info"
table_id_start = 100
"#;
    let (_dir, path) = create_temp_config(content);

    let result = ConfigLoader::load(&path);

    assert!(result.is_ok());
    let config = result.unwrap();
    assert_eq!(config.global.monitor_interval, 5);
}

#[test]
fn test_load_config_with_invalid_table_id_fails() {
    let content = r#"
[global]
monitor_interval = 5
table_id_start = 50
"#;
    let (_dir, path) = create_temp_config(content);

    let result = ConfigLoader::load(&path);

    assert!(result.is_err());
}

#[test]
fn test_hot_reload_updates_config() {
    let original = r#"
[global]
monitor_interval = 5
"#;
    let updated = r#"
[global]
monitor_interval = 10
"#;

    let (_dir, path) = create_temp_config(original);
    let mut loader = ConfigLoader::new(&path);
    loader.load().unwrap();

    // 修改配置文件
    fs::write(&path, updated).unwrap();

    // 触发重载
    let config = loader.reload().unwrap();
    assert_eq!(config.global.monitor_interval, 10);
}
```

### 共享测试工具

```rust
// tests/common/mod.rs

pub mod mock_network {
    use std::collections::HashMap;
    use nic_autoswitch::monitor::{NetworkState, InterfaceInfo};

    pub fn create_mock_state() -> NetworkState {
        NetworkState {
            interfaces: HashMap::from([
                ("eth0".to_string(), InterfaceInfo {
                    name: "eth0".to_string(),
                    interface_type: InterfaceType::Lan,
                    addresses: vec!["192.168.1.100/24".parse().unwrap()],
                    is_up: true,
                }),
                ("wlan0".to_string(), InterfaceInfo {
                    name: "wlan0".to_string(),
                    interface_type: InterfaceType::Wlan,
                    addresses: vec!["10.0.0.50/24".parse().unwrap()],
                    is_up: true,
                    ssid: Some("CorpWiFi".to_string()),
                }),
            ]),
        }
    }
}

pub mod fixtures {
    pub const VALID_CONFIG: &str = include_str!("../fixtures/valid_config.toml");
    pub const INVALID_CONFIG: &str = include_str!("../fixtures/invalid_config.toml");
}
```

## Mock 策略

### 使用 trait 抽象

```rust
// src/monitor/mod.rs
pub trait NetworkMonitor: Send + Sync {
    fn current_state(&self) -> NetworkState;
    fn events(&self) -> BoxStream<'_, NetworkEvent>;
}

// 生产实现
pub struct NetlinkMonitor { /* ... */ }
impl NetworkMonitor for NetlinkMonitor { /* ... */ }

// Mock 实现
#[cfg(test)]
pub struct MockNetworkMonitor {
    state: NetworkState,
    events: VecDeque<NetworkEvent>,
}

#[cfg(test)]
impl NetworkMonitor for MockNetworkMonitor {
    fn current_state(&self) -> NetworkState {
        self.state.clone()
    }

    fn events(&self) -> BoxStream<'_, NetworkEvent> {
        // 返回预设的事件流
    }
}

#[cfg(test)]
impl MockNetworkMonitor {
    pub fn add_event(&mut self, event: NetworkEvent) {
        self.events.push_back(event);
    }
}
```

### 系统调用 Mock

对于 netlink、D-Bus 等系统调用，使用 feature flag 控制是否 mock：

```rust
// Cargo.toml
[features]
default = []
test-mock = []

// src/monitor/netlink.rs
#[cfg(not(feature = "test-mock"))]
pub fn create_connection() -> Result<Connection> {
    rtnetlink::new_connection()
}

#[cfg(feature = "test-mock")]
pub fn create_connection() -> Result<MockConnection> {
    Ok(MockConnection::new())
}
```

## 测试覆盖率

### 覆盖率目标

| 类型 | 目标 | 最低要求 |
|------|------|----------|
| 行覆盖率 (Line) | **80%** | 70% |
| 分支覆盖率 (Branch) | **75%** | 60% |
| 关键路径 | **100%** | 100% |

### 关键路径定义

以下代码必须 100% 覆盖：

- 路由规则匹配逻辑 (`engine/matcher.rs`)
- 路由规则执行 (`engine/executor.rs`)
- 配置验证 (`config/validator.rs`)
- 错误处理路径

### 运行覆盖率测试

```bash
# 安装 tarpaulin
cargo install cargo-tarpaulin

# 运行覆盖率测试
cargo tarpaulin --out Html --out Stdout

# 指定输出目录
cargo tarpaulin --out Html --output-dir coverage/

# 只测试特定模块
cargo tarpaulin --lib --skip-files "tests/*"
```

### CI 集成

```yaml
# .github/workflows/ci.yml
- name: Run coverage
  run: cargo tarpaulin --out Xml --output-dir coverage/

- name: Upload to Codecov
  uses: codecov/codecov-action@v4
  with:
    files: coverage/cobertura.xml
    fail_ci_if_error: true
```

## 测试最佳实践

### 1. 使用 AAA 模式

```rust
#[test]
fn test_apply_rule() {
    // Arrange (准备)
    let mut router = Router::new();
    let rule = RouteRule::default();

    // Act (执行)
    let result = router.apply_rule(&rule);

    // Assert (断言)
    assert!(result.is_ok());
}
```

### 2. 测试边界条件

```rust
#[test]
fn test_cidr_match_boundary() {
    let cidr: IpNetwork = "10.0.0.0/8".parse().unwrap();

    // 边界内
    assert!(cidr.contains(&"10.0.0.0".parse().unwrap()));
    assert!(cidr.contains(&"10.255.255.255".parse().unwrap()));

    // 边界外
    assert!(!cidr.contains(&"9.255.255.255".parse().unwrap()));
    assert!(!cidr.contains(&"11.0.0.0".parse().unwrap()));
}
```

### 3. 测试错误情况

```rust
#[test]
fn test_parse_invalid_config_returns_descriptive_error() {
    let result = parse_config("invalid [[[");

    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.to_string().contains("line 1"));  // 错误消息包含位置信息
}
```

### 4. 避免测试实现细节

```rust
// ❌ 不好：测试内部实现
#[test]
fn test_internal_vec_length() {
    let matcher = RuleMatcher::new();
    assert_eq!(matcher.rules.len(), 0);  // 内部实现细节
}

// ✅ 好：测试公开行为
#[test]
fn test_matcher_with_no_rules_returns_none() {
    let matcher = RuleMatcher::new();
    let result = matcher.match_destination(&Destination::default());
    assert!(result.is_none());
}
```
