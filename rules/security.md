# 安全规范

本文档定义 nic-autoswitch 项目的安全要求和最佳实践。

## 权限要求

### 必需权限

| 权限 | 用途 | 获取方式 |
|------|------|----------|
| `CAP_NET_ADMIN` | 操作路由表、网络接口 | systemd capability 或 root |
| `CAP_NET_RAW` | 原始套接字（netlink） | systemd capability 或 root |
| 文件读取 | 配置文件、日志 | 普通用户 |

### systemd 服务配置

```ini
# systemd/nic-autoswitch.service
[Unit]
Description=Network Interface Auto-Switch Daemon
After=network-online.target
Wants=network-online.target

[Service]
Type=notify
ExecStart=/usr/bin/nic-autoswitch --config /etc/nic-autoswitch/config.toml

# 安全加固
CapabilityBoundingSet=CAP_NET_ADMIN CAP_NET_RAW
AmbientCapabilities=CAP_NET_ADMIN CAP_NET_RAW

# 限制文件系统访问
ReadOnlyPaths=/etc
ReadWritePaths=/run/nic-autoswitch
NoNewPrivileges=yes

# 限制系统调用
SystemCallFilter=@system-service
SystemCallArchitectures=native

# 资源限制
LimitNOFILE=1024
MemoryMax=100M

[Install]
WantedBy=multi-user.target
```

## 敏感信息处理

### 禁止硬编码凭据

```rust
// ❌ 绝对禁止
const API_KEY: &str = "sk-xxxx";
const PASSWORD: &str = "secret123";

// ✅ 从安全来源读取
let api_key = std::env::var("NIC_AUTOSWITCH_API_KEY")?;
let password = std::env::var("NIC_AUTOSWITCH_PASSWORD")?;
```

### 日志中隐藏敏感信息

```rust
// ✅ 好：隐藏部分 SSID
pub fn sanitize_ssid(ssid: &str) -> String {
    if ssid.len() > 4 {
        format!("{}****", &ssid[..4])
    } else {
        "****".to_string()
    }
}

info!(ssid = %sanitize_ssid(&ssid), "WiFi connected");

// ❌ 禁止：记录敏感信息
info!(password = %password, "Auth success");  // 绝对禁止
info!(api_key = %key, "API configured");      // 绝对禁止
```

### 配置文件权限

```bash
# 配置文件权限（如包含敏感信息）
chmod 600 /etc/nic-autoswitch/config.toml
chown root:root /etc/nic-autoswitch/config.toml

# 运行时目录
chmod 770 /run/nic-autoswitch
chown root:nic-autoswitch /run/nic-autoswitch
```

## 输入验证

### 配置文件验证

```rust
pub fn validate_config(config: &Config) -> Result<(), ConfigError> {
    // 验证接口名称（防止注入）
    for (name, iface) in &config.interfaces {
        if !is_valid_interface_name(name) {
            return Err(ConfigError::InvalidValue {
                field: format!("interfaces.{}.name", name),
                reason: "Invalid interface name format".to_string(),
            });
        }

        // 验证优先级范围
        if iface.priority > 10000 {
            return Err(ConfigError::InvalidValue {
                field: format!("interfaces.{}.priority", name),
                reason: "Priority must be <= 10000".to_string(),
            });
        }
    }

    // 验证 CIDR 格式
    for rule in &config.routing.default_rules {
        if let MatchOn::Cidr(cidr) = &rule.match_on {
            validate_cidr(cidr)?;
        }
    }

    Ok(())
}

fn is_valid_interface_name(name: &str) -> bool {
    // Linux 接口名规范：1-15 字符，字母数字下划线
    name.len() >= 1
        && name.len() <= 15
        && name.chars().all(|c| c.is_alphanumeric() || c == '_')
}
```

### CLI 参数验证

```rust
use clap::Parser;

#[derive(Parser)]
struct Args {
    /// 配置文件路径
    #[arg(short, long, value_parser = validate_path)]
    config: PathBuf,
}

fn validate_path(path: &str) -> Result<PathBuf, String> {
    let path = PathBuf::from(path);

    // 检查路径是否在允许的目录内
    let canonical = path.canonicalize()
        .map_err(|e| format!("Invalid path: {}", e))?;

    if !canonical.starts_with("/etc/nic-autoswitch/")
        && !canonical.starts_with("/usr/local/etc/nic-autoswitch/") {
        return Err("Config file must be in /etc/nic-autoswitch/ or /usr/local/etc/nic-autoswitch/".to_string());
    }

    Ok(canonical)
}
```

### 域名验证

```rust
pub fn validate_domain(domain: &str) -> Result<(), ValidationError> {
    // 检查长度
    if domain.len() > 253 {
        return Err(ValidationError::DomainTooLong);
    }

    // 检查字符
    for label in domain.split('.') {
        if label.is_empty() || label.len() > 63 {
            return Err(ValidationError::InvalidDomainFormat);
        }

        if !label.chars().all(|c| c.is_alphanumeric() || c == '-') {
            return Err(ValidationError::InvalidDomainChar);
        }
    }

    Ok(())
}
```

## 错误消息安全

### 不泄露内部信息

```rust
// ✅ 好：通用错误消息
pub fn connect(&self) -> Result<()> {
    self.inner_connect().map_err(|e| {
        tracing::debug!(error = %e, "Connection failed");  // 详细错误记录到日志
        NicAutoSwitchError::Network("Connection failed".to_string())  // 用户看到通用消息
    })
}

// ❌ 避免：泄露内部路径
pub fn load(&self, path: &Path) -> Result<Config> {
    let content = fs::read_to_string(path)?;  // 错误可能包含内部路径
}

// ✅ 好：处理后返回
pub fn load(&self, path: &Path) -> Result<Config> {
    let content = fs::read_to_string(path)
        .map_err(|_| ConfigError::NotFound("Config file".to_string()))?;
}
```

## 网络安全

### Unix Socket 权限

```rust
use std::os::unix::fs::PermissionsExt;

pub fn create_control_socket(path: &Path) -> Result<UnixListener> {
    // 创建目录
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
        fs::set_permissions(parent, fs::Permissions::from_mode(0o770))?;
    }

    // 绑定 socket
    let listener = UnixListener::bind(path)?;

    // 设置 socket 权限
    fs::set_permissions(path, fs::Permissions::from_mode(0o660))?;

    Ok(listener)
}
```

### D-Bus 安全

```rust
// 仅连接系统总线，不暴露服务
let conn = zbus::Connection::system().await?;

// 如果需要暴露服务，添加安全策略
// /etc/dbus-1/system.d/nic-autoswitch.conf
```

### Netlink 安全

```rust
// 仅订阅必要的事件组
let (conn, handle, _) = rtnetlink::new_connection()?;
conn.socket_mut()?
    .add_membership(rtnl_groups::RTNLGRP_IPV4_ROUTE)?;
conn.socket_mut()?
    .add_membership(rtnlgroups::RTNLGRP_LINK)?;
// 不添加不必要的组
```

## 依赖安全

### 定期审计

```bash
# 安装 cargo-audit
cargo install cargo-audit

# 运行审计
cargo audit

# CI 集成
cargo audit --deny warnings
```

### 最小化依赖

```bash
# 检查依赖树
cargo tree

# 查看重复依赖
cargo tree --duplicates

# 只启用必要的 features
tokio = { version = "1", features = ["rt-multi-thread", "macros", "sync"] }
# 而非
tokio = { version = "1", features = ["full"] }
```

## 安全检查清单

### 开发阶段

- [ ] 无硬编码凭据或密钥
- [ ] 输入验证完整
- [ ] 错误消息不泄露敏感信息
- [ ] 配置文件权限正确
- [ ] 日志不记录敏感信息

### 部署阶段

- [ ] systemd 服务配置正确
- [ ] 文件权限最小化
- [ ] 网络访问受限
- [ ] 日志目录权限正确

### 维护阶段

- [ ] 定期运行 cargo audit
- [ ] 关注安全公告
- [ ] 及时更新有漏洞的依赖

## 安全事件响应

### 发现漏洞时

1. **评估严重性**
   - CVSS 评分
   - 影响范围

2. **修复流程**
   - 创建安全分支
   - 修复漏洞
   - 内部测试

3. **发布流程**
   - 发布安全版本
   - 更新 CHANGELOG
   - 通知用户

### 版本号规则

```
安全修复增加 PATCH 版本
0.1.0 → 0.1.1 (安全修复)
```
