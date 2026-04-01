# 依赖管理规范

本文档定义 nic-autoswitch 项目的依赖管理策略。

## 添加依赖检查清单

在添加新依赖前，必须完成以下检查：

- [ ] **必要性**：是否真正需要？能否用标准库或现有依赖解决？
- [ ] **维护状态**：项目是否活跃维护？（最近 6 个月有提交）
- [ ] **安全性**：运行 `cargo audit` 检查已知漏洞
- [ ] **最小化**：只启用需要的 features
- [ ] **兼容性**：与现有依赖版本兼容
- [ ] **体积影响**：评估对编译时间和二进制大小的影响

## 依赖分类

### 生产依赖

```toml
[dependencies]
# 异步运行时
tokio = { version = "1", features = ["rt-multi-thread", "macros", "signal", "time", "sync", "net"] }

# 日志
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter", "fmt"] }

# 配置
config = "0.14"
serde = { version = "1", features = ["derive"] }
toml = "0.8"

# 错误处理
anyhow = "1"
thiserror = "2"
```

### 开发依赖

```toml
[dev-dependencies]
tokio-test = "0.4"
tempfile = "3"
mockall = "0.13"
criterion = { version = "0.5", features = ["async_tokio"] }
```

### 构建依赖

```toml
[build-dependencies]
# 仅在构建时需要
```

## 版本指定策略

### 推荐做法

```toml
# 使用兼容版本范围
tokio = "1"                    # 1.x 最新版
serde = { version = "1" }      # 1.x 最新版
tracing = "0.1"                # 0.1.x 最新版

# 需要特定特性时指定最小版本
tokio = { version = "1.35", features = [...] }  # 需要 1.35+ 的特性
```

### 避免的做法

```toml
# ❌ 避免使用 * 通配符
tokio = "*"                    # 不确定版本

# ❌ 避免过于精确的版本
tokio = "1.35.1"               # 限制了补丁更新

# ❌ 避免不必要的 git 依赖
tokio = { git = "https://github.com/tokio-rs/tokio" }  # 除非有特殊需求
```

## Features 管理

### 最小化 Features

```toml
# ✅ 好：只启用需要的 features
tokio = { version = "1", features = ["rt-multi-thread", "macros", "sync"] }

# ❌ 避免：启用所有 features
tokio = { version = "1", features = ["full"] }
```

### 项目 Features

```toml
[features]
default = ["systemd"]
systemd = ["tracing-journald"]
test-mock = []
```

## 定期维护

### 检查命令

```bash
# 查看过时依赖
cargo outdated

# 安全漏洞检查
cargo audit

# 依赖树分析
cargo tree

# 查看重复依赖
cargo tree --duplicates

# 编译时间分析
cargo bloat --time

# 二进制大小分析
cargo bloat --release
```

### 更新策略

```bash
# 更新补丁版本（推荐定期执行）
cargo update

# 更新特定依赖
cargo update -p tokio

# 检查 Major 版本更新
cargo outdated --root-deps-only
```

## 依赖审查流程

### 添加新依赖时

1. **评估需求**
   ```bash
   # 搜索 crates.io
   # 比较同类 crate
   # 查看文档和示例
   ```

2. **检查质量**
   ```bash
   # GitHub stars, issues
   # 下载量
   # 最近更新时间
   # 依赖数量
   ```

3. **安全审计**
   ```bash
   cargo audit
   ```

4. **测试影响**
   ```bash
   # 添加前
   cargo build --timings

   # 添加后
   cargo build --timings

   # 比较编译时间
   ```

### 常用依赖质量指标

| 指标 | 好的迹象 | 警告信号 |
|------|----------|----------|
| 下载量 | > 10k/天 | < 100/天 |
| GitHub Stars | > 1000 | < 100 |
| 最近更新 | < 3 个月 | > 1 年 |
| Open Issues | < 50 | > 500 |
| 依赖数 | < 10 | > 50 |

## 本项目依赖清单

### 核心依赖

| 依赖 | 版本 | 用途 |
|------|------|------|
| tokio | 1.x | 异步运行时 |
| tracing | 0.1.x | 日志框架 |
| config | 0.14.x | 配置管理 |
| serde | 1.x | 序列化 |
| anyhow | 1.x | 错误处理 |
| thiserror | 2.x | 错误定义 |

### 网络相关

| 依赖 | 版本 | 用途 |
|------|------|------|
| rtnetlink | 0.14.x | Netlink 通信 |
| zbus | 5.x | D-Bus 通信 |
| hickory-resolver | 0.24.x | DNS 解析 |
| ipnetwork | 0.20.x | IP 网络处理 |

### 工具依赖

| 依赖 | 版本 | 用途 |
|------|------|------|
| clap | 4.x | CLI 参数解析 |
| notify | 6.x | 文件监控 |
| signal-hook | 0.3.x | 信号处理 |

### 开发依赖

| 依赖 | 版本 | 用途 |
|------|------|------|
| tempfile | 3.x | 临时文件/目录 |
| tokio-test | 0.4.x | 异步测试工具 |
| mockall | 0.13.x | Mock 框架 |
| criterion | 0.5.x | 性能基准测试 |
| cargo-tarpaulin | 0.31.x | 测试覆盖率 |

## Cargo.lock 管理

- **应用项目**：提交 `Cargo.lock` 到版本控制
- **库项目**：不提交 `Cargo.lock`

本项目是应用，因此：

```bash
# 确保 Cargo.lock 被跟踪
git add Cargo.lock
```

## 安全最佳实践

1. **定期运行 cargo audit**
   ```bash
   # 安装
   cargo install cargo-audit

   # 运行
   cargo audit
   ```

2. **CI 中集成**
   ```yaml
   - name: Security audit
     run: cargo audit
   ```

3. **关注安全公告**
   - RustSec Advisory Database
   - GitHub Dependabot alerts

4. **最小权限原则**
   - 检查 build.rs 脚本
   - 注意 proc-macro 的能力
