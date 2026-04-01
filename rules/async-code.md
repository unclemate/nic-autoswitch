# 异步代码规范

本文档定义 nic-autoswitch 项目中使用 tokio 异步运行时的最佳实践。

## tokio 运行时配置

### 主函数

```rust
#[tokio::main]
async fn main() -> Result<()> {
    // 应用代码
    Ok(())
}

// 或者自定义配置
fn main() -> Result<()> {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(4)
        .enable_all()
        .build()?
        .block_on(async {
            // 应用代码
            Ok(())
        })
}
```

### 运行时配置建议

| 场景 | 配置 |
|------|------|
| CPU 密集型 | `worker_threads = CPU 核心数` |
| I/O 密集型（本项目） | `worker_threads = 2-4` |
| 低延迟要求 | `max_blocking_threads = 较小值` |

## 异步函数命名

```rust
// ✅ 好：不添加 _async 后缀
pub async fn apply_rules(&self) -> Result<()> { }
pub async fn resolve_dns(&self, domain: &str) -> Result<Vec<IpAddr>> { }

// ❌ 避免：添加 _async 后缀
pub async fn apply_rules_async(&self) -> Result<()> { }
```

## 并发控制

### tokio::select!

```rust
use tokio::select;
use tokio::signal::unix::{signal, SignalKind};

async fn run_service(
    mut shutdown: tokio::sync::watch::Receiver<bool>,
) -> Result<()> {
    let mut sigterm = signal(SignalKind::terminate())?;
    let mut sighup = signal(SignalKind::hangup())?;

    loop {
        select! {
            // 事件处理
            Some(event) = event_receiver.recv() => {
                handle_event(event).await?;
            }

            // 信号处理
            _ = sigterm.recv() => {
                info!("Received SIGTERM, shutting down");
                break;
            }

            _ = sighup.recv() => {
                info!("Received SIGHUP, reloading config");
                reload_config().await?;
            }

            // 关闭信号
            _ = shutdown.changed() => {
                info!("Shutdown requested");
                break;
            }
        }
    }

    Ok(())
}
```

### tokio::join! 并行执行

```rust
// 并行执行，等待全部完成
async fn initialize() -> Result<(Config, Monitor, Router)> {
    let config_future = load_config();
    let monitor_future = create_monitor();
    let router_future = create_router();

    let (config, monitor, router) = tokio::join!(
        config_future,
        monitor_future,
        router_future,
    );

    Ok((config?, monitor?, router?))
}
```

### tokio::try_join! 并行执行（任一失败立即返回）

```rust
async fn health_check(&self) -> Result<HealthStatus> {
    let (netlink, dbus, dns) = tokio::try_join!(
        self.check_netlink(),
        self.check_dbus(),
        self.check_dns(),
    )?;

    Ok(HealthStatus {
        netlink,
        dbus,
        dns,
    })
}
```

## 避免阻塞

### 使用 spawn_blocking

```rust
// ❌ 避免：在异步代码中直接阻塞
pub async fn load_config(&self) -> Result<Config> {
    let content = std::fs::read_to_string(&self.path)?;  // 阻塞！
    // ...
}

// ✅ 好：使用 spawn_blocking
pub async fn load_config(&self) -> Result<Config> {
    let path = self.path.clone();
    let content = tokio::task::spawn_blocking(move || {
        std::fs::read_to_string(&path)
    }).await??;

    let config: Config = toml::from_str(&content)?;
    Ok(config)
}

// ✅ 更好：使用 tokio 异步 API
pub async fn load_config(&self) -> Result<Config> {
    let content = tokio::fs::read_to_string(&self.path).await?;
    let config: Config = toml::from_str(&content)?;
    Ok(config)
}
```

### 常见阻塞点

| 阻塞操作 | 替代方案 |
|----------|----------|
| `std::fs::*` | `tokio::fs::*` |
| `std::net::*` | `tokio::net::*` |
| `std::time::sleep` | `tokio::time::sleep` |
| `std::sync::Mutex` | `tokio::sync::Mutex` |
| CPU 密集计算 | `spawn_blocking` |

## 锁的选择

### std::sync vs tokio::sync

```rust
// ❌ 避免：在异步代码中使用 std::sync::Mutex
use std::sync::Mutex;

async fn update_state(&self, event: Event) {
    let guard = self.state.lock().unwrap();  // 可能阻塞整个运行时
    guard.apply(event);
}

// ✅ 好：使用 tokio::sync::Mutex
use tokio::sync::Mutex;

async fn update_state(&self, event: Event) {
    let guard = self.state.lock().await;
    guard.apply(event);
}
```

### 锁类型选择

| 场景 | 推荐锁类型 |
|------|-----------|
| 短时间持有（< 1ms） | `tokio::sync::Mutex` |
| 长时间持有 | `std::sync::Mutex` + `spawn_blocking` |
| 读多写少 | `tokio::sync::RwLock` |
| 简单状态通知 | `tokio::sync::watch` |
| 多生产者多消费者 | `tokio::sync::broadcast` |
| 单生产者单消费者 | `tokio::sync::mpsc` |

### 示例：状态管理

```rust
use tokio::sync::{Mutex, watch, broadcast};

pub struct ServiceState {
    // 共享状态
    pub config: Mutex<Config>,

    // 状态变更通知（单播）
    pub state_change: watch::Sender<ServiceStatus>,

    // 事件广播（多播）
    pub events: broadcast::Sender<NetworkEvent>,
}
```

## 超时和取消

### 超时控制

```rust
use tokio::time::{timeout, Duration};

// 单个操作超时
pub async fn resolve_with_timeout(&self, domain: &str) -> Result<Vec<IpAddr>> {
    timeout(
        Duration::from_secs(5),
        self.resolver.lookup_ip(domain),
    )
    .await
    .map_err(|_| NicAutoSwitchError::Timeout { timeout_ms: 5000 })??
}

// 多个操作总超时
pub async fn batch_resolve(&self, domains: &[&str]) -> Result<Vec<Vec<IpAddr>>> {
    let total_timeout = Duration::from_secs(30);

    timeout(total_timeout, async {
        let mut results = Vec::new();
        for domain in domains {
            results.push(self.resolve(domain).await?);
        }
        Ok(results)
    })
    .await?
}
```

### 取消处理

```rust
use tokio_util::sync::CancellationToken;

pub struct Service {
    cancel_token: CancellationToken,
}

impl Service {
    pub async fn run(&self) -> Result<()> {
        loop {
            select! {
                _ = self.cancel_token.cancelled() => {
                    info!("Service cancelled");
                    break;
                }

                event = self.recv_event() => {
                    self.handle_event(event?).await?;
                }
            }
        }
        Ok(())
    }

    pub fn shutdown(&self) {
        self.cancel_token.cancel();
    }
}
```

## 错误处理

### 在 select! 中处理错误

```rust
loop {
    select! {
        result = event_receiver.recv() => {
            match result {
                Some(event) => handle_event(event).await?,
                None => {
                    // 通道关闭
                    warn!("Event channel closed");
                    break;
                }
            }
        }

        result = async_operation() => {
            if let Err(e) = result {
                error!(error = %e, "Async operation failed");
                // 决定是否继续
            }
        }
    }
}
```

## 测试异步代码

```rust
#[tokio::test]
async fn test_resolve_domain() {
    let resolver = DnsResolver::new();

    let result = resolver.resolve("example.com").await;

    assert!(result.is_ok());
}

#[tokio::test(start_paused = true)]  // 时间不自动前进
async fn test_timeout() {
    let resolver = DnsResolver::new();

    // 模拟超时
    tokio::time::pause();
    tokio::time::advance(Duration::from_secs(6)).await;

    let result = resolver.resolve_with_timeout("slow.example.com").await;
    assert!(matches!(result, Err(NicAutoSwitchError::Timeout { .. })));
}
```

## 异步最佳实践清单

- [ ] 使用 `tokio::fs` 而非 `std::fs`
- [ ] 使用 `tokio::sync::Mutex` 而非 `std::sync::Mutex`（在 .await 点持有锁时）
- [ ] 避免在异步代码中阻塞
- [ ] 使用 `select!` 处理并发事件
- [ ] 设置合理的超时时间
- [ ] 实现 graceful shutdown
- [ ] 异步函数不添加 `_async` 后缀
- [ ] 使用 `#[tokio::test]` 测试异步代码
