# nic-autoswitch 开发规范

本目录包含 nic-autoswitch 项目的完整开发规范文档。

## 快速导航

### 核心规范

| 文档 | 描述 |
|------|------|
| [命名规范](naming.md) | snake_case/PascalCase 等命名约定 |
| [设计原则](principles.md) | DRY/KISS/YAGNI/SOLID 原则应用 |
| [TDD 开发规范](tdd.md) | 测试驱动开发策略和实践 |
| [测试规范](testing.md) | 单元测试、集成测试、覆盖率要求 |

### 代码质量

| 文档 | 描述 |
|------|------|
| [文档注释](documentation.md) | rustdoc 格式和示例 |
| [错误处理](error-handling.md) | thiserror/anyhow 使用规范 |
| [日志规范](logging.md) | tracing 日志级别和使用 |
| [异步代码](async-code.md) | tokio 最佳实践 |
| [代码审查](code-review.md) | 审查清单和标准 |

### 项目管理

| 文档 | 描述 |
|------|------|
| [分支管理](branching.md) | GitHub Flow + Conventional Commits |
| [版本管理](versioning.md) | 语义化版本 + CHANGELOG |
| [依赖管理](dependencies.md) | 依赖添加检查清单 |

### 运维和安全

| 文档 | 描述 |
|------|------|
| [性能基准](performance.md) | 性能指标和基准测试 |
| [安全规范](security.md) | 权限、输入验证、敏感信息处理 |
| [工具配置](tooling.md) | pre-commit + GitHub Actions |

## 规范摘要

### 命名规范速查

```
模块/函数/变量: snake_case
类型/结构体/枚举: PascalCase
常量: SCREAMING_SNAKE_CASE
文件: snake_case.rs
```

### 测试覆盖率目标

```
行覆盖率: 80% (最低 70%)
分支覆盖率: 75% (最低 60%)
关键路径: 100%
```

### Commit 格式

```
<type>(<scope>): <description>

类型: feat/fix/docs/test/refactor/perf/style/chore/ci
范围: config/monitor/router/engine/daemon/cli/deps/release
```

### 分支策略

```
main        → 稳定版本
feature/*   → 功能开发
fix/*       → Bug 修复
release/*   → 版本发布
```

## 使用指南

### 开始开发前

1. 阅读 [命名规范](naming.md) 和 [设计原则](principles.md)
2. 安装 pre-commit hooks：
   ```bash
   pip install pre-commit
   pre-commit install
   pre-commit install --hook-type commit-msg
   ```
3. 熟悉 [分支管理](branching.md) 流程

### 提交代码前

1. 运行本地检查：
   ```bash
   cargo fmt
   cargo clippy -- -D warnings
   cargo test
   ```
2. 参考 [代码审查清单](code-review.md)
3. 遵循 [Commit 格式](branching.md#commit-规范)

### 创建 PR 时

1. 确保 CI 通过
2. 参考 [代码审查清单](code-review.md)
3. 更新相关文档

## 规范更新

规范文档应随项目发展持续更新。修改规范时：

1. 在团队讨论后更新
2. 在 PR 中说明变更原因
3. 更新本 README 的摘要（如有必要）

## 参考资源

- [Rust API Guidelines](https://rust-lang.github.io/api-guidelines/)
- [The Rust Programming Language](https://doc.rust-lang.org/book/)
- [Tokio Tutorial](https://tokio.rs/tokio/tutorial)
- [Semantic Versioning](https://semver.org/)
- [Conventional Commits](https://www.conventionalcommits.org/)
- [Keep a Changelog](https://keepachangelog.com/)
