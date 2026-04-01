# 错误处理规范

本文档定义 nic-autoswitch 项目的错误处理策略和最佳实践。

## 错误类型定义

### 项目级错误枚举

使用 `thiserror` 定义统一的错误类型：

```rust
// src/error.rs

use thiserror::Error;

/// nic-autoswitch 主错误类型
#[derive(Debug, Error)]
pub enum NicAutoSwitchError {
    /// 配置相关错误
    #[error("Configuration error: {0}")]
    Config(#[from] ConfigError),

    /// 网络操作错误
    #[error("Network operation failed: {context}")]
    Network {
        context: String,
        #[source]
        source: Option<std::io::Error>,
    },

    /// DNS 解析错误
    #[error("DNS resolution failed for '{domain}': {reason}")]
    Dns {
        domain: String,
        reason: String,
    },

    /// 路由操作错误
    #[error("Route operation failed: {0}")]
    Route(String),

    /// I/O 错误
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// D-Bus 通信错误
    #[error("D-Bus error: {0}")]
    Dbus(String),

    /// 通道通信错误
    #[error("Channel error: {0}")]
    Channel(String),

    /// 无效输入
    #[error("Invalid input: {0}")]
    InvalidInput(String),

    /// 操作超时
    #[error("Operation timed out after {timeout_ms}ms")]
    Timeout {
        timeout_ms: u64,
    },
}

/// 配置错误子类型
#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("Config file not found: {0}")]
    NotFound(String),

    #[error("Config parse error at line {line}: {message}")]
    ParseError {
        line: usize,
        message: String,
    },

    #[error("Config validation failed: {0}")]
    Validation(String),

    #[error("Invalid value for '{field}': {reason}")]
    InvalidValue {
        field: String,
        reason: String,
    },
}

/// 项目级 Result 类型别名
pub type Result<T> = std::result::Result<T, NicAutoSwitchError>;
```

## 错误处理原则

### 1. 使用 `?` 操作符传播错误

```rust
// ✅ 好
pub fn load_config(path: &Path) -> Result<Config> {
    let content = fs::read_to_string(path)?;  // 自动转换为 NicAutoSwitchError::Io
    let config: Config = toml::from_str(&content)
        .map_err(|e| ConfigError::ParseError {
            line: e.line_col().map(|(l, _)| l + 1).unwrap_or(0),
            message: e.to_string(),
        })?;
    Ok(config)
}

// ❌ 避免：不必要的 match
pub fn load_config(path: &Path) -> Result<Config> {
    match fs::read_to_string(path) {
        Ok(content) => {
            match toml::from_str(&content) {
                Ok(config) => Ok(config),
                Err(e) => Err(NicAutoSwitchError::from(e)),
            }
        }
        Err(e) => Err(NicAutoSwitchError::from(e)),
    }
}
```

### 2. 使用 `map_err` 添加上下文

```rust
// ✅ 好：添加上下文信息
pub fn resolve_domain(&self, domain: &str) -> Result<Vec<IpAddr>> {
    self.resolver
        .lookup_ip(domain)
        .await
        .map_err(|e| NicAutoSwitchError::Dns {
            domain: domain.to_string(),
            reason: e.to_string(),
        })?
        .into_iter()
        .collect()
}

// ❌ 避免：错误信息不足
pub fn resolve_domain(&self, domain: &str) -> Result<Vec<IpAddr>> {
    self.resolver.lookup_ip(domain).await?  // 丢失了域名信息
}
```

### 3. 避免 `unwrap()` 和 `expect()` 在生产代码中

```rust
// ✅ 好：处理可能的错误
pub fn get_interface(&self, name: &str) -> Result<&InterfaceInfo> {
    self.interfaces
        .get(name)
        .ok_or_else(|| NicAutoSwitchError::InvalidInput(
            format!("Interface '{}' not found", name)
        ))
}

// ⚠️ 仅在测试中使用 expect
#[cfg(test)]
fn test_something() {
    let config = Config::default();
    assert_eq!(config.global.monitor_interval, 5);
}

// ⚠️ 仅在逻辑上不可能失败时使用 expect
fn main() {
    let args = Args::parse();
    if args.config_path.exists() {
        // 这里我们知道文件存在，expect 是合理的
        let contents = fs::read_to_string(&args.config_path)
            .expect("File exists check passed but read failed");
    }
}
```

### 4. 使用 `context()` 扩展错误信息

```rust
// 如果使用 anyhow
use anyhow::{Context, Result};

pub fn load_config(path: &Path) -> Result<Config> {
    let content = fs::read_to_string(path)
        .with_context(|| format!("Failed to read config file: {}", path.display()))?;

    let config: Config = toml::from_str(&content)
        .with_context(|| format!("Failed to parse config file: {}", path.display()))?;

    Ok(config)
}
```

## 应用层 vs 库层错误处理

### 库层（lib）使用 `thiserror`

```rust
// src/lib.rs
pub use error::{NicAutoSwitchError, Result};

// 返回具体错误类型
pub fn parse_config(s: &str) -> Result<Config> {
    // ...
}
```

### 应用层（main）可使用 `anyhow`

```rust
// src/main.rs
use anyhow::Result;

fn main() -> Result<()> {
    // 应用层可以使用 anyhow 处理错误
    let config = load_config(&args.config)?;

    // 启动服务
    run_service(config).await?;

    Ok(())
}

async fn run_service(config: Config) -> Result<()> {
    // 应用层错误处理
    let monitor = create_monitor(&config)
        .context("Failed to create network monitor")?;

    // ...
    Ok(())
}
```

## 错误恢复策略

### 1. 重试机制

```rust
use std::time::Duration;
use tokio::time::sleep;

pub async fn connect_with_retry<F, T, E>(
    mut f: F,
    max_retries: u32,
    base_delay: Duration,
) -> Result<T>
where
    F: FnMut() -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<T>>>>,
    E: std::fmt::Display,
{
    let mut retries = 0;

    loop {
        match f().await {
            Ok(result) => return Ok(result),
            Err(e) if retries < max_retries => {
                retries += 1;
                let delay = base_delay * (1 << retries.min(5));  // 指数退避，最大 32x
                tracing::warn!(
                    error = %e,
                    retry = retries,
                    delay_ms = delay.as_millis(),
                    "Operation failed, retrying"
                );
                sleep(delay).await;
            }
            Err(e) => return Err(e),
        }
    }
}
```

### 2. 优雅降级

```rust
pub async fn get_wifi_ssid(&self, interface: &str) -> Option<String> {
    // 首选：通过 NetworkManager D-Bus
    match self.dbus_get_ssid(interface).await {
        Ok(ssid) => return Some(ssid),
        Err(e) => {
            tracing::debug!("D-Bus SSID query failed: {}", e);
        }
    }

    // 降级：通过 iw 命令
    match self.iw_get_ssid(interface).await {
        Ok(ssid) => return Some(ssid),
        Err(e) => {
            tracing::debug!("iw SSID query failed: {}", e);
        }
    }

    // 无法获取 SSID，但不影响核心功能
    tracing::info!("Could not determine WiFi SSID for {}", interface);
    None
}
```

### 3. 错误分类处理

```rust
pub fn handle_error(error: &NicAutoSwitchError) -> ErrorAction {
    match error {
        // 可恢复错误：记录日志，继续运行
        NicAutoSwitchError::Dns { .. } => {
            tracing::warn!("{}", error);
            ErrorAction::Continue
        }

        // 需要重试的错误
        NicAutoSwitchError::Network { .. } => {
            ErrorAction::RetryAfter(Duration::from_secs(5))
        }

        // 致命错误：需要停止
        NicAutoSwitchError::Config(ConfigError::NotFound(_)) => {
            tracing::error!("{}", error);
            ErrorAction::Fatal
        }

        // 默认：记录并继续
        _ => {
            tracing::error!("{}", error);
            ErrorAction::Continue
        }
    }
}

pub enum ErrorAction {
    Continue,
    RetryAfter(Duration),
    Fatal,
}
```

## 日志中的错误处理

```rust
use tracing::{error, warn, info, debug, instrument};

#[instrument(skip(self), fields(interface = %interface))]
pub async fn handle_interface_event(&self, interface: &str) -> Result<()> {
    match self.apply_rules_for_interface(interface).await {
        Ok(_) => {
            info!("Rules applied successfully");
            Ok(())
        }
        Err(e) => {
            // 结构化错误日志
            error!(
                error = %e,
                error_chain = ?e,  // 包含完整错误链
                "Failed to apply rules"
            );

            // 决定是否传播错误
            if is_recoverable(&e) {
                warn!("Recoverable error, continuing...");
                Ok(())
            } else {
                Err(e)
            }
        }
    }
}
```

## 错误测试

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_not_found_error() {
        let result = load_config(Path::new("/nonexistent/config.toml"));

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, NicAutoSwitchError::Io(_)));
    }

    #[test]
    fn test_invalid_config_parse_error() {
        let result = parse_config("invalid [[[[");

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, NicAutoSwitchError::Config(ConfigError::ParseError { .. })));
    }

    #[test]
    fn test_error_display_contains_context() {
        let err = NicAutoSwitchError::Dns {
            domain: "example.com".to_string(),
            reason: "timeout".to_string(),
        };

        let message = err.to_string();
        assert!(message.contains("example.com"));
        assert!(message.contains("timeout"));
    }
}
```
