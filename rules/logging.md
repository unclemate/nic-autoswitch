# 日志规范

本文档定义 nic-autoswitch 项目的日志记录策略和最佳实践。

## 日志级别

| 级别 | 使用场景 | 示例 |
|------|----------|------|
| `ERROR` | 严重错误，需要立即关注 | 配置加载失败、关键服务崩溃 |
| `WARN` | 警告，潜在问题但系统继续运行 | D-Bus 连接失败降级模式、配置项缺失使用默认值 |
| `INFO` | 重要业务事件 | 服务启动/停止、路由规则变更、WiFi 连接变化 |
| `DEBUG` | 详细调试信息 | 规则匹配结果、DNS 查询详情 |
| `TRACE` | 最详细的跟踪信息 | 数据包内容、函数入口/出口 |

## 日志初始化

```rust
use tracing_subscriber::{
    layer::SubscriberExt,
    util::SubscriberInitExt,
    EnvFilter,
};

pub fn init_logging(log_level: &str) -> Result<()> {
    // 环境变量覆盖：RUST_LOG=debug
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(log_level));

    // journald 集成（systemd 环境）
    #[cfg(target_os = "linux")]
    {
        if std::env::var("INVOCATION_ID").is_ok() {
            // 在 systemd 下运行，使用 journald
            let journal = tracing_journald::layer()?;
            tracing_subscriber::registry()
                .with(filter)
                .with(journal)
                .init();
            return Ok(());
        }
    }

    // 回退到标准输出
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .with_thread_ids(false)
        .compact()
        .init();

    Ok(())
}
```

## 日志记录模式

### 基本日志

```rust
use tracing::{error, warn, info, debug, trace};

// 基本日志
info!("Service started");
warn!("Config file not found, using defaults");
error!("Failed to connect to netlink socket");

// 带字段的日志
info!(
    interface = %interface_name,
    table_id = route.table_id,
    "Route rule applied"
);
```

### 使用 `instrument` 宏

```rust
use tracing::instrument;

// 自动记录函数入口和出口
#[instrument(skip(self))]
pub async fn apply_rule(&self, rule: &RouteRule) -> Result<()> {
    // 函数体
    Ok(())
}

// 输出：
// INFO apply_rule{rule=RouteRule { ... }}: Entered
// INFO apply_rule{rule=RouteRule { ... }}:Exited

// 指定记录的字段
#[instrument(skip(self, config), fields(interface = %rule.interface))]
pub async fn apply_rule(&self, rule: &RouteRule, config: &Config) -> Result<()> {
    // ...
}

// 记录返回值
#[instrument(ret, err)]
pub async fn resolve(&self, domain: &str) -> Result<Vec<IpAddr>> {
    // ...
}
```

### 结构化日志字段

```rust
// ✅ 好：使用结构化字段
info!(
    interface.name = %interface_name,
    interface.type = ?interface_type,
    route.count = rules.len(),
    "Applied routing rules"
);

// ❌ 避免：字符串拼接
info!(format!("Applied {} rules for interface {}", rules.len(), interface_name));
```

## 关键事件日志

### 服务生命周期

```rust
// 启动
info!(
    version = %env!("CARGO_PKG_VERSION"),
    config = %config_path.display(),
    "nic-autoswitch starting"
);

// 就绪
info!(
    interfaces = ?active_interfaces,
    "Service ready, monitoring network events"
);

// 停止
info!(
    reason = %reason,
    uptime_secs = start_time.elapsed().as_secs(),
    "Service shutting down"
);
```

### 网络事件

```rust
// 接口状态变化
info!(
    interface = %name,
    old_state = ?old_state,
    new_state = ?new_state,
    "Interface state changed"
);

// WiFi 连接
info!(
    interface = %interface,
    ssid = %ssid,
    bssid = %bssid,
    "WiFi connected"
);

// 路由规则变更
info!(
    interface = %interface,
    rules.added = added_count,
    rules.removed = removed_count,
    "Route table updated"
);
```

### 错误日志

```rust
// 可恢复错误
warn!(
    error = %e,
    interface = %interface,
    "DNS resolution failed, using cached result"
);

// 严重错误
error!(
    error = %e,
    error.chain = ?e,  // 完整错误链
    "Critical error in route manager"
);

// 操作失败
error!(
    operation = "apply_route",
    interface = %interface,
    error = %e,
    "Operation failed"
);
```

## 日志性能考虑

### 避免昂贵的计算

```rust
// ✅ 好：日志级别检查
if tracing::debug_enabled!() {
    let detailed_state = self.compute_expensive_debug_info();
    debug!(state = ?detailed_state, "Current state");
}

// ❌ 避免：总是计算
debug!(state = ?self.compute_expensive_debug_info(), "Current state");
```

### 使用 `skip` 跳过大字段

```rust
#[instrument(skip(self, large_data))]
pub fn process(&self, large_data: &[u8]) -> Result<()> {
    // ...
}
```

### 异步日志

```rust
// tracing 默认是同步的，对于高吞吐量场景
// 考虑使用 tracing-appender
use tracing_appender::non_blocking::NonBlocking;
use tracing_subscriber::fmt::Layer;

let (non_blocking, _guard) = tracing_appender::non_blocking(std::io::stdout());

tracing_subscriber::registry()
    .with(Layer::default().with_writer(non_blocking))
    .init();
```

## 日志格式

### 开发环境

```
2026-03-27T10:30:45.123456Z INFO nic_autoswitch::daemon: Service started version="0.1.0"
2026-03-27T10:30:45.234567Z INFO nic_autoswitch::monitor: WiFi connected interface="wlan0" ssid="CorpWiFi"
```

### 生产环境（journald）

```
Mar 27 10:30:45 hostname nic-autoswitch[1234]: Service started
Mar 27 10:30:45 hostname nic-autoswitch[1234]: WiFi connected
```

## 日志查看

### journalctl

```bash
# 实时查看
journalctl -u nic-autoswitch -f

# 过滤级别
journalctl -u nic-autoswitch -p err

# 按时间范围
journalctl -u nic-autoswitch --since "1 hour ago"

# JSON 输出
journalctl -u nic-autoswitch -o json
```

### 文件日志

```bash
# tail
tail -f /var/log/nic-autoswitch.log

# grep
grep "ERROR" /var/log/nic-autoswitch.log
```

## 敏感信息处理

```rust
// ✅ 好：隐藏敏感信息
pub fn sanitize_ssid(ssid: &str) -> String {
    if ssid.len() > 4 {
        format!("{}****", &ssid[..4])
    } else {
        "****".to_string()
    }
}

info!(
    ssid = %sanitize_ssid(&ssid),
    "WiFi connected"
);

// ❌ 避免：记录敏感信息
info!(password = %config.password, "Config loaded");  // 绝对不要这样做
```

## 日志规范检查清单

- [ ] 使用结构化日志字段
- [ ] 关键操作有 INFO 级别日志
- [ ] 错误有 ERROR 或 WARN 级别日志
- [ ] 不记录敏感信息
- [ ] 大对象使用 `skip` 或条件日志
- [ ] 异步函数使用 `#[instrument]`
- [ ] 错误日志包含错误链
