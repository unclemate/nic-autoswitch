# TDD 开发规范

本文档定义 nic-autoswitch 项目的测试驱动开发（Test-Driven Development）策略和实践。

## 概述

nic-autoswitch 采用 **分层 TDD 策略**：根据代码与系统交互的程度，采用不同的测试策略。

## TDD 适用范围

### ✅ 强制 TDD（必须先写测试）

这些模块是纯逻辑，输入输出明确，非常适合 TDD：

| 模块 | 文件 | 原因 |
|------|------|------|
| 配置数据结构 | `config/schema.rs` | 数据验证逻辑，无副作用 |
| 配置加载器 | `config/loader.rs` | 解析逻辑，可用字符串 fixture |
| 规则匹配引擎 | `engine/matcher.rs` | 核心业务逻辑，纯函数 |
| 错误类型 | `error.rs` | 简单类型转换 |

**判定标准：**
- 无 I/O 操作
- 无系统调用
- 确定性输出
- 可用内存数据构造输入

### ⚠️ 推荐 TDD（使用 Mock）

这些模块需要抽象接口后使用 mock 进行 TDD：

| 模块 | 文件 | Mock 策略 |
|------|------|-----------|
| 路由管理器 | `router/manager.rs` | trait `RouteOperator` |
| DNS 解析器 | `router/dns.rs` | trait `DnsResolver` |
| 网络监控 | `monitor/state.rs` | trait `NetworkMonitor` |

**实现方式：**

```rust
// 定义 trait 抽象
pub trait RouteOperator: Send + Sync {
    fn add_route(&self, route: &Route) -> Result<()>;
    fn remove_route(&self, route: &Route) -> Result<()>;
}

// 生产实现
pub struct NetlinkRouteOperator { /* ... */ }
impl RouteOperator for NetlinkRouteOperator { /* ... */ }

// 测试 Mock
#[cfg(test)]
pub struct MockRouteOperator {
    routes: Mutex<Vec<Route>>,
}
impl RouteOperator for MockRouteOperator { /* ... */ }
```

### 📝 后补测试（先实现后测试）

这些模块依赖系统环境，TDD 成本高，建议先实现后测试：

| 模块 | 文件 | 测试策略 |
|------|------|----------|
| Netlink 监控 | `monitor/netlink.rs` | 集成测试 + 手动测试 |
| D-Bus 监控 | `monitor/networkmanager.rs` | 手动测试 |
| 信号处理 | `daemon/signals.rs` | 集成测试 |
| 主服务 | `daemon/service.rs` | 端到端测试 |

**判定标准：**
- 依赖外部系统（内核、D-Bus、systemd）
- 需要 root 权限
- 环境难以模拟

---

## TDD 工作流程

### 标准 Red-Green-Refactor 循环

```
┌─────────────────────────────────────────────────────────┐
│                    TDD 循环                              │
│                                                          │
│   1. 🔴 RED                                              │
│      └─→ 编写一个失败的测试                               │
│          - 测试应描述期望行为                             │
│          - 编译失败也算失败                               │
│                                                          │
│   2. 🟢 GREEN                                            │
│      └─→ 编写最少代码使测试通过                           │
│          - 不追求完美，只需通过                           │
│          - 可以硬编码返回值                               │
│                                                          │
│   3. 🔵 REFACTOR                                         │
│      └─→ 重构代码，消除重复                               │
│          - 测试保证行为不变                               │
│          - 应用 DRY/KISS 原则                            │
│                                                          │
│   └─→ 重复循环                                           │
└─────────────────────────────────────────────────────────┘
```

### 具体步骤

#### Step 1: 编写测试（RED）

```rust
// src/engine/matcher.rs

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_match_destination_with_cidr_returns_matching_rule() {
        // Arrange
        let rules = vec![RouteRule {
            name: "corp-cidr".into(),
            match_on: MatchOn::Cidr("10.0.0.0/8".parse().unwrap()),
            route_via: "eth0".into(),
            priority: 100,
        }];
        let matcher = RuleMatcher::new(rules);
        let dest = Destination::Ip("10.1.2.3".parse().unwrap());

        // Act
        let result = matcher.match_destination(&dest);

        // Assert
        assert!(result.is_some());
        assert_eq!(result.unwrap().name, "corp-cidr");
    }
}
```

运行测试，确认失败：
```bash
$ cargo test
error[E0425]: cannot find value `RouteRule` in this scope
```

#### Step 2: 实现代码（GREEN）

```rust
// src/engine/matcher.rs

pub struct RouteRule {
    pub name: String,
    pub match_on: MatchOn,
    pub route_via: String,
    pub priority: u32,
}

pub enum MatchOn {
    Cidr(IpNetwork),
}

pub struct RuleMatcher {
    rules: Vec<RouteRule>,
}

impl RuleMatcher {
    pub fn new(rules: Vec<RouteRule>) -> Self {
        Self { rules }
    }

    pub fn match_destination(&self, dest: &Destination) -> Option<&RouteRule> {
        // 最简实现：遍历所有规则
        self.rules.iter().find(|rule| {
            matches!(&rule.match_on, MatchOn::Cidr(cidr) if cidr.contains(dest.ip()))
        })
    }
}
```

运行测试，确认通过：
```bash
$ cargo test
test test_match_destination_with_cidr_returns_matching_rule ... ok
```

#### Step 3: 重构（REFACTOR）

```rust
// 应用 DRY 原则，提取匹配逻辑
impl RouteRule {
    pub fn matches(&self, dest: &Destination) -> bool {
        match &self.match_on {
            MatchOn::Cidr(cidr) => cidr.contains(dest.ip()),
            MatchOn::Ip(ip) => dest.ip() == ip,
            MatchOn::Domain(domain) => dest.domain() == Some(domain),
            // 添加新匹配类型无需修改 matcher 逻辑
        }
    }
}

impl RuleMatcher {
    pub fn match_destination(&self, dest: &Destination) -> Option<&RouteRule> {
        self.rules.iter()
            .find(|rule| rule.matches(dest))
    }
}
```

---

## 测试命名规范

### 格式

```rust
// 格式: test_<方法名>_<场景>_<期望结果>
#[test]
fn test_match_destination_with_cidr_returns_matching_rule() {}

#[test]
fn test_load_config_with_invalid_toml_returns_error() {}

#[test]
fn test_resolve_domain_with_cached_entry_returns_cached() {}
```

### 场景描述词

| 场景 | 示例 |
|------|------|
| 正常情况 | `with_valid_config` |
| 边界条件 | `at_boundary`, `with_empty_list` |
| 错误情况 | `with_invalid_input`, `when_not_found` |
| 特殊状态 | `when_cached`, `when_disconnected` |

### 期望结果词

| 结果 | 示例 |
|------|------|
| 成功 | `returns_true`, `succeeds` |
| 失败 | `returns_error`, `fails` |
| 返回值 | `returns_matching_rule`, `returns_none` |

---

## 测试组织

### 内联单元测试

```rust
// src/config/schema.rs

pub struct Config {
    pub global: GlobalConfig,
    pub interfaces: HashMap<String, InterfaceConfig>,
}

impl Config {
    pub fn validate(&self) -> Result<(), ValidationError> {
        self.global.validate()?;
        for iface in self.interfaces.values() {
            iface.validate()?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_with_empty_interfaces_returns_error() {
        let config = Config {
            global: GlobalConfig::default(),
            interfaces: HashMap::new(),
        };
        assert!(config.validate().is_err());
    }
}
```

### 测试 Fixtures

```rust
// tests/common/fixtures.rs

pub fn create_test_config() -> Config {
    Config {
        global: GlobalConfig {
            monitor_interval: 5,
            log_level: "info".into(),
            table_id_start: 100,
        },
        interfaces: HashMap::from([
            ("eth0".into(), InterfaceConfig {
                interface_type: InterfaceType::Lan,
                priority: 10,
                ..Default::default()
            }),
        ]),
    }
}

pub fn create_test_rules() -> Vec<RouteRule> {
    vec![
        RouteRule {
            name: "corp-cidr".into(),
            match_on: MatchOn::Cidr("10.0.0.0/8".parse().unwrap()),
            route_via: "eth0".into(),
            priority: 100,
        },
    ]
}
```

---

## Mock 策略

### 使用 trait 抽象

```rust
// src/router/mod.rs

/// 路由操作接口 - 用于依赖注入
pub trait RouteOperator: Send + Sync {
    fn add_route(&self, route: &Route) -> Result<()>;
    fn remove_route(&self, route: &Route) -> Result<()>;
    fn list_routes(&self) -> Result<Vec<Route>>;
}

// src/router/manager.rs - 生产实现
pub struct NetlinkRouteOperator {
    connection: netlink::Connection,
}

impl RouteOperator for NetlinkRouteOperator {
    fn add_route(&self, route: &Route) -> Result<()> {
        // 真实的 netlink 调用
    }
}

// src/router/mock.rs - 测试 Mock
#[cfg(test)]
pub struct MockRouteOperator {
    routes: Arc<Mutex<Vec<Route>>>,
}

#[cfg(test)]
impl RouteOperator for MockRouteOperator {
    fn add_route(&self, route: &Route) -> Result<()> {
        self.routes.lock().unwrap().push(route.clone());
        Ok(())
    }
}
```

### 使用 mockall crate

```rust
// Cargo.toml
[dev-dependencies]
mockall = "0.13"

// tests/integration/router_test.rs
use mockall::automock;

#[automock]
pub trait DnsResolver: Send + Sync {
    async fn resolve(&self, domain: &str) -> Result<Vec<IpAddr>>;
}

#[tokio::test]
async fn test_match_domain_rule_resolves_and_matches() {
    let mut mock_resolver = MockDnsResolver::new();
    mock_resolver
        .expect_resolve()
        .with(eq("gitlab.corp.com"))
        .times(1)
        .returning(|_| Ok(vec!["10.1.2.3".parse().unwrap()]));

    let matcher = RuleMatcher::new(
        Box::new(mock_resolver),
        create_test_rules(),
    );

    let result = matcher.match_destination(&Destination::Domain("gitlab.corp.com".into())).await;
    assert!(result.is_some());
}
```

---

## TDD 检查清单

### 开始编码前

- [ ] 确认模块属于哪种 TDD 范围（强制/推荐/后补）
- [ ] 对于强制 TDD 模块：先写测试
- [ ] 对于推荐 TDD 模块：先定义 trait 接口

### 每个功能点

- [ ] 🔴 编写失败的测试
- [ ] 🟢 编写最少代码使测试通过
- [ ] 🔵 重构，确保测试仍通过
- [ ] 测试命名符合规范
- [ ] 覆盖正常、边界、错误场景

### 提交前

- [ ] `cargo test` 全部通过
- [ ] 新代码有对应测试
- [ ] 测试覆盖率达标（80% 目标）

---

## TDD 示例：实现 CIDR 匹配器

### 第一轮：基本匹配

```rust
// 🔴 RED: 编写测试
#[test]
fn test_match_with_matching_cidr_returns_true() {
    let matcher = CidrMatcher::new("10.0.0.0/8".parse().unwrap());
    assert!(matcher.matches(&"10.1.2.3".parse().unwrap()));
}

// 🟢 GREEN: 最简实现
pub struct CidrMatcher {
    cidr: IpNetwork,
}

impl CidrMatcher {
    pub fn matches(&self, ip: &IpAddr) -> bool {
        self.cidr.contains(ip)
    }
}

// 🔵 REFACTOR: 暂无需重构
```

### 第二轮：非匹配情况

```rust
// 🔴 RED: 编写测试
#[test]
fn test_match_with_non_matching_cidr_returns_false() {
    let matcher = CidrMatcher::new("10.0.0.0/8".parse().unwrap());
    assert!(!matcher.matches(&"8.8.8.8".parse().unwrap()));
}

// 🟢 GREEN: 已通过（ipnetwork 库已正确实现）

// 🔵 REFACTOR: 无需修改
```

### 第三轮：IPv6 支持

```rust
// 🔴 RED: 编写测试
#[test]
fn test_match_with_ipv6_cidr_works_correctly() {
    let matcher = CidrMatcher::new("2001:db8::/32".parse().unwrap());
    assert!(matcher.matches(&"2001:db8::1".parse().unwrap()));
    assert!(!matcher.matches(&"2001:db9::1".parse().unwrap()));
}

// 🟢 GREEN: 已通过（ipnetwork 库支持 IPv6）

// 🔵 REFACTOR: 添加文档
impl CidrMatcher {
    /// 检查 IP 地址是否在 CIDR 范围内
    ///
    /// 支持 IPv4 和 IPv6 地址
    pub fn matches(&self, ip: &IpAddr) -> bool {
        self.cidr.contains(ip)
    }
}
```

---

## 参考资源

- [Test-Driven Development with Rust](https://rust-lang.github.io/rust-by-example/testing.html)
- [Mockall Documentation](https://docs.rs/mockall/)
- [Rust Testing Patterns](https://matklad.github.io/2021/05/31/how-to-test.html)
