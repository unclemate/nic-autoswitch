# 版本管理规范

本文档定义 nic-autoswitch 项目的版本号策略和变更日志规范。

## 语义化版本

遵循 [Semantic Versioning 2.0.0](https://semver.org/) 规范。

### 版本格式

```
MAJOR.MINOR.PATCH[-PRERELEASE][+BUILD]

示例：
1.0.0
1.0.1
1.1.0
2.0.0
1.0.0-alpha.1
1.0.0-beta.2
1.0.0-rc.1
```

### 版本递增规则

| 变更类型 | 版本变化 | 示例 |
|----------|----------|------|
| 不兼容的 API 变更 | MAJOR++ | 1.0.0 → 2.0.0 |
| 向后兼容的功能新增 | MINOR++ | 1.0.0 → 1.1.0 |
| 向后兼容的问题修复 | PATCH++ | 1.0.0 → 1.0.1 |
| 预发布版本 | PRERELEASE | 1.0.0-alpha.1 → 1.0.0-alpha.2 |

### 版本变更示例

```toml
# Cargo.toml
[package]
version = "0.1.0"  # 初始版本

# 添加新功能 → 0.2.0
# 修复 bug → 0.1.1
# 破坏性变更 → 0.2.0 或 1.0.0（取决于是否稳定）
```

## 版本判断指南

### MAJOR 版本（主版本）

以下情况增加 MAJOR：

- 移除公共 API
- 重命名公共 API
- 修改配置文件格式（不向后兼容）
- 修改 CLI 参数（不向后兼容）
- 修改默认行为（影响现有用户）

```rust
// 示例：破坏性变更
// 之前
pub fn apply_rule(rule: &RouteRule) -> Result<()>

// 之后（破坏性）
pub fn apply_rule(rule: RouteRule, options: ApplyOptions) -> Result<()>
```

### MINOR 版本（次版本）

以下情况增加 MINOR：

- 添加新的公共 API
- 添加新的配置项
- 添加新的 CLI 命令
- 性能优化
- 扩展功能（不破坏现有行为）

```rust
// 示例：向后兼容的功能新增
// 现有 API 保持不变
pub fn apply_rule(rule: &RouteRule) -> Result<()>

// 新增 API
pub fn apply_rules(rules: &[RouteRule]) -> Result<()>
```

### PATCH 版本（补丁版本）

以下情况增加 PATCH：

- Bug 修复
- 文档更新
- 内部重构（不影响 API）
- 依赖更新（安全修复）

```rust
// 示例：Bug 修复
// 修复前（有 bug）
pub fn is_private_ip(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => v4.is_private(),
        // 忘记处理 IPv6
    }
}

// 修复后
pub fn is_private_ip(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => v4.is_private(),
        IpAddr::V6(v6) => v6.is_unique_local(),  // 修复
    }
}
```

## 预发布版本

### 预发布阶段

| 阶段 | 标识 | 用途 |
|------|------|------|
| Alpha | `alpha` | 内部测试，功能不完整 |
| Beta | `beta` | 公开测试，功能基本完整 |
| Release Candidate | `rc` | 发布候选，仅修复 bug |

### 预发布版本号

```toml
# Alpha 版本
version = "1.0.0-alpha.1"
version = "1.0.0-alpha.2"

# Beta 版本
version = "1.0.0-beta.1"
version = "1.0.0-beta.2"

# Release Candidate
version = "1.0.0-rc.1"
version = "1.0.0-rc.2"

# 正式版本
version = "1.0.0"
```

## 变更日志（CHANGELOG）

### 格式

遵循 [Keep a Changelog](https://keepachangelog.com/) 规范。

```markdown
# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/),
and this project adheres to [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added
- 新功能（尚未发布）

## [0.2.0] - 2026-03-27

### Added
- IPv6 dual-stack support for routing tables
- DNS AAAA record resolution with caching
- Configuration hot-reload functionality
- CLI `show dns-cache` command

### Changed
- Improved rule matching performance by 50%
- Updated minimum Rust version to 1.70

### Fixed
- Netlink socket reconnection handling
- DNS cache invalidation on network change
- Memory leak in event dispatcher

### Security
- Fixed potential path traversal in config loader (CVE-2026-XXXX)

## [0.1.1] - 2026-03-15

### Fixed
- Graceful shutdown handling
- Log level parsing error

## [0.1.0] - 2026-03-01

### Added
- Initial release
- Network interface monitoring via netlink
- WiFi SSID monitoring via D-Bus
- Route table management
- DNS resolution with caching
- Configuration file support
- CLI tool for status and control
- systemd service integration
```

### 变更类型

| 类型 | 描述 |
|------|------|
| `Added` | 新功能 |
| `Changed` | 现有功能的变更 |
| `Deprecated` | 即将移除的功能 |
| `Removed` | 已移除的功能 |
| `Fixed` | Bug 修复 |
| `Security` | 安全修复 |

## 发布流程

### 准备发布

```bash
# 1. 确保在 main 分支
git checkout main
git pull origin main

# 2. 更新版本号
# - Cargo.toml
# - Cargo.lock (cargo build)
# - systemd service 文件（如有路径变更）

# 3. 更新 CHANGELOG.md
# - 移动 Unreleased 内容到新版本
# - 添加发布日期

# 4. 运行完整测试
cargo test --all-features
cargo clippy -- -D warnings
cargo fmt --check

# 5. 提交版本更新
git add -A
git commit -m "chore(release): prepare v0.2.0"

# 6. 推送并创建 PR
git push origin main
```

### 创建发布

```bash
# 1. 创建并推送 tag
git tag -a v0.2.0 -m "Release v0.2.0"
git push origin v0.2.0

# 2. 在 GitHub 创建 Release
# - 使用 tag v0.2.0
# - 标题：v0.2.0
# - 内容：CHANGELOG 对应部分

# 3. 发布到 crates.io（可选）
cargo publish
```

### 版本支持

| 版本 | 状态 | 支持期限 |
|------|------|----------|
| 0.2.x | 当前版本 | 持续支持 |
| 0.1.x | 维护模式 | 仅安全修复 |
| < 0.1.0 | 不支持 | 已停止 |

## Git Tag 规范

### Tag 命名

```bash
# 正式版本
v0.1.0
v0.1.1
v1.0.0

# 预发布版本
v1.0.0-alpha.1
v1.0.0-beta.1
v1.0.0-rc.1
```

### Tag 消息

```bash
git tag -a v0.2.0 -m "Release v0.2.0

New features:
- IPv6 dual-stack support
- Configuration hot-reload

Bug fixes:
- Netlink reconnection handling
- DNS cache invalidation
"
```

## 自动化版本检查

### CI 配置

```yaml
# .github/workflows/release.yml
name: Release Check

on:
  pull_request:
    paths:
      - 'Cargo.toml'

jobs:
  check-version:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
        - name: Check version bump
        run: |
          # 获取当前版本
          CURRENT=$(git show origin/main:Cargo.toml | grep '^version' | head -1 | cut -d'"' -f2)
          # 获取 PR 版本
          PR=$(grep '^version' Cargo.toml | head -1 | cut -d'"' -f2)

          echo "Current: $CURRENT, PR: $PR"

          # 检查版本是否增加
          # ... 版本比较逻辑
```
