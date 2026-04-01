# 设计原则

本文档定义 nic-autoswitch 项目遵循的软件设计原则，确保代码可维护、可扩展、易理解。

## DRY - Don't Repeat Yourself

**原则**：每项知识在系统中必须有单一、明确的表示。

### 应用示例

#### 配置验证逻辑抽取

```rust
// ❌ 不好：验证逻辑散落各处
impl Config {
    fn validate(&self) -> Result<()> {
        if self.table_id_start < 100 {
            return Err("table_id_start must >= 100");
        }
    }
}

impl WifiProfile {
    fn validate(&self) -> Result<()> {
        if self.rules.is_empty() {
            return Err("rules cannot be empty");
        }
    }
}

// ✅ 好：抽取通用验证器
// src/config/validator.rs
pub trait Validate {
    fn validate(&self) -> Result<(), ValidationError>;
}

pub fn validate_range<T: PartialOrd>(value: T, min: T, max: T, name: &str) -> Result<(), ValidationError> {
    if value < min || value > max {
        return Err(ValidationError::OutOfRange { field: name.to_string() });
    }
    Ok(())
}

impl Config {
    fn validate(&self) -> Result<(), ValidationError> {
        validate_range(self.table_id_start, 100, 250, "table_id_start")?;
        Ok(())
    }
}
```

#### IP 地址处理工具

```rust
// ❌ 不好：重复的 IP 处理逻辑
fn is_ipv4(ip: &str) -> bool { ip.contains('.') }
fn is_ipv6(ip: &str) -> bool { ip.contains(':') }

// ✅ 好：集中到工具模块
// src/utils/ip.rs
pub fn parse_ip_network(s: &str) -> Result<IpNetwork, ParseError> {
    s.parse()
}

pub fn is_private_ip(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => v4.is_private(),
        IpAddr::V6(v6) => v6.is_unique_local(),
    }
}
```

---

## KISS - Keep It Simple, Stupid

**原则**：追求简洁，避免不必要的复杂性。

### 应用示例

#### 简单的错误处理

```rust
// ❌ 不好：过度设计的错误处理
trait ErrorHandler: Send + Sync {
    fn handle(&self, error: Box<dyn std::error::Error>);
}

struct CompositeErrorHandler {
    handlers: Vec<Box<dyn ErrorHandler>>,
}

// ✅ 好：简单直接
pub type Result<T> = std::result::Result<T, NicAutoSwitchError>;
```

#### 避免过度抽象

```rust
// ❌ 不好：为未来可能的需求预留扩展点
trait RouteStrategy {
    fn route(&self, packet: &Packet) -> Option<Interface>;
}

struct WeightedRoundRobinStrategy { }
struct LeastConnectionStrategy { }

// ✅ 好：只实现当前需要的功能
pub fn select_interface_for_destination(dest: &IpAddr, rules: &[RouteRule]) -> Option<&str> {
    rules.iter()
        .find(|r| r.matches(dest))
        .map(|r| r.interface.as_str())
}
```

#### 优先使用标准库

```rust
// ❌ 不好：引入第三方库做简单事情
use itertools::Itertools;  // 仅为了 chain

// ✅ 好：使用标准库
rules.iter().chain(default_rules.iter())
```

---

## YAGNI - You Aren't Gonna Need It

**原则**：只实现当前明确需要的功能，不做未来假设。

### 应用示例

#### 不预留未使用的配置项

```toml
# ❌ 不好：预留"可能用到"的配置
[global]
monitor_interval = 5
log_level = "info"
future_feature_x = false  # 暂未实现
advanced_mode = false     # 暂未实现

# ✅ 好：只配置已实现的功能
[global]
monitor_interval = 5
log_level = "info"
```

#### 不实现未请求的功能

```rust
// ❌ 不好：实现"可能有用"的功能
impl Router {
    pub fn load_balance(&self) { }  // 未请求
    pub fn qos_support(&self) { }   // 未请求
}

// ✅ 好：只实现设计文档中的功能
impl Router {
    pub fn apply_rule(&self, rule: &RouteRule) -> Result<()> { }
    pub fn remove_rule(&self, rule_id: &str) -> Result<()> { }
}
```

---

## SOLID 原则

### S - 单一职责原则 (Single Responsibility Principle)

**原则**：一个模块只做一件事。

```
✅ 正确的模块划分：
├── monitor/     → 只负责监控网络状态
├── router/      → 只负责管理路由表
├── engine/      → 只负责规则匹配和执行
├── daemon/      → 只负责守护进程生命周期
└── config/      → 只负责配置管理

❌ 避免：
├── network/     → 既监控又路由，职责不清
```

### O - 开放封闭原则 (Open-Closed Principle)

**原则**：对扩展开放，对修改封闭。

```rust
// ✅ 通过 trait 扩展匹配规则
pub trait Matcher {
    fn matches(&self, target: &Destination) -> bool;
}

pub struct CidrMatcher { cidr: IpNetwork }
impl Matcher for CidrMatcher {
    fn matches(&self, target: &Destination) -> bool {
        match target {
            Destination::Ip(ip) => self.cidr.contains(ip),
            _ => false,
        }
    }
}

pub struct DomainMatcher { pattern: String }
impl Matcher for DomainMatcher {
    fn matches(&self, target: &Destination) -> bool {
        match target {
            Destination::Domain(d) => d.ends_with(&self.pattern),
            _ => false,
        }
    }
}

// 添加新的匹配器无需修改现有代码
pub struct RegexMatcher { regex: Regex }
impl Matcher for RegexMatcher { /* ... */ }
```

### L - 里氏替换原则 (Liskov Substitution Principle)

**原则**：子类型可以替换父类型而不影响程序正确性。

```rust
// ✅ 所有 Matcher 实现可互换使用
pub fn find_matching_rule(rules: &[Box<dyn Matcher>], target: &Destination) -> Option<usize> {
    rules.iter().position(|m| m.matches(target))
}

// 任何 Matcher 实现都可以传入此函数
```

### I - 接口隔离原则 (Interface Segregation Principle)

**原则**：接口要小而专一，避免"胖接口"。

```rust
// ❌ 不好：一个大接口
pub trait NetworkManager {
    fn monitor(&self) -> Events;
    fn apply_route(&self, route: Route);
    fn resolve_dns(&self, domain: &str);
    fn handle_signal(&self, sig: Signal);
}

// ✅ 好：拆分为专一的小接口
pub trait NetworkMonitor {
    fn events(&self) -> Events;
}

pub trait RouteManager {
    fn apply(&self, route: Route) -> Result<()>;
}

pub trait DnsResolver {
    fn resolve(&self, domain: &str) -> Result<Vec<IpAddr>>;
}
```

### D - 依赖倒置原则 (Dependency Inversion Principle)

**原则**：依赖抽象（trait），而非具体实现。

```rust
// ❌ 不好：直接依赖具体实现
pub struct Service {
    netlink_monitor: NetlinkMonitor,  // 具体
    dbus_client: zbus::Connection,    // 具体
}

// ✅ 好：依赖抽象
pub struct Service {
    monitor: Box<dyn NetworkMonitor>,  // 抽象
    resolver: Box<dyn DnsResolver>,    // 抽象
}

// 便于测试时注入 mock
#[cfg(test)]
struct MockMonitor;
impl NetworkMonitor for MockMonitor { /* ... */ }

let service = Service {
    monitor: Box::new(MockMonitor),
    // ...
};
```

---

## 原则优先级

当原则发生冲突时，按以下优先级处理：

1. **KISS** - 简洁性优先
2. **YAGNI** - 不做过度设计
3. **DRY** - 在简洁的前提下消除重复
4. **SOLID** - 在复杂度合理的情况下遵循
