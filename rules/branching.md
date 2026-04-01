# 分支管理规范

本文档定义 nic-autoswitch 项目的 Git 分支策略、命名规则和提交流程。

## 分支策略：GitHub Flow

采用简化的 GitHub Flow，适合持续部署场景。

```
main ─────●─────●─────●─────●─────●─────→ (生产就绪)
           \   /       \   /
feature/a   ●─●         ●─●  feature/b
            ↑           ↑
         PR合并      PR合并
```

### 分支类型

| 分支类型 | 命名规则 | 用途 | 生命周期 |
|----------|----------|------|----------|
| `main` | `main` | 稳定版本，随时可发布 | 永久 |
| `feature/*` | `feature/<描述>` | 新功能开发 | 合并后删除 |
| `fix/*` | `fix/<描述>` | Bug 修复 | 合并后删除 |
| `docs/*` | `docs/<描述>` | 文档更新 | 合并后删除 |
| `refactor/*` | `refactor/<描述>` | 代码重构 | 合并后删除 |
| `release/*` | `release/v<版本>` | 发布准备 | 发布后删除 |

### 分支命名示例

```bash
# 功能分支
feature/ipv6-dual-stack
feature/wifi-ssid-matching
feature/config-hot-reload

# 修复分支
fix/netlink-reconnect
fix/dns-cache-invalidation
fix/memory-leak-in-matcher

# 文档分支
docs/api-documentation
docs/installation-guide

# 重构分支
refactor/rule-matcher
refactor/error-handling
```

## Commit 规范

采用 [Conventional Commits](https://www.conventionalcommits.org/) 规范。

### 格式

```
<type>(<scope>): <description>

[optional body]

[optional footer(s)]
```

### Type 类型

| Type | 用途 | 示例 |
|------|------|------|
| `feat` | 新功能 | `feat(router): add IPv6 dual-stack support` |
| `fix` | Bug 修复 | `fix(monitor): handle netlink reconnect gracefully` |
| `docs` | 文档更新 | `docs(readme): update installation instructions` |
| `test` | 测试相关 | `test(config): add validation tests` |
| `refactor` | 代码重构 | `refactor(engine): simplify rule matching logic` |
| `perf` | 性能优化 | `perf(dns): add LRU cache for DNS resolution` |
| `style` | 代码格式 | `style: fix clippy warnings` |
| `chore` | 杂项 | `chore(deps): update dependencies` |
| `ci` | CI 配置 | `ci: add coverage report workflow` |

### Scope 范围

根据项目模块划分：

| Scope | 模块 |
|-------|------|
| `config` | 配置管理 |
| `monitor` | 网络监控 |
| `router` | 路由管理 |
| `engine` | 核心引擎 |
| `daemon` | 守护进程 |
| `cli` | CLI 工具 |
| `deps` | 依赖管理 |
| `release` | 版本发布 |

### Commit 示例

```bash
# 新功能
feat(router): add IPv6 dual-stack support

- Add IPv6 route table management
- Support AAAA record DNS resolution
- Update rule matching for IPv6 addresses

Closes #42

# Bug 修复
fix(monitor): handle netlink socket disconnection

The netlink monitor would panic when the socket was closed
unexpectedly. Now it attempts to reconnect with exponential
backoff.

Fixes #89

# 破坏性变更
feat(config)!: change config file format to YAML

BREAKING CHANGE: Configuration files must now use YAML format
instead of TOML. Use the migration tool to convert existing configs.
```

## PR (Pull Request) 流程

### 创建 PR

```bash
# 1. 从 main 创建功能分支
git checkout main
git pull origin main
git checkout -b feature/ipv6-support

# 2. 开发并提交
git add .
git commit -m "feat(router): add IPv6 route table management"
git push origin feature/ipv6-support

# 3. 在 GitHub 创建 PR
```

### PR 标题格式

```
<type>(<scope>): <description>
```

示例：
- `feat(router): add IPv6 dual-stack support`
- `fix(monitor): handle netlink reconnect`

### PR 描述模板

```markdown
## 变更说明
<!-- 简要描述此 PR 的目的 -->

## 变更类型
- [ ] 新功能 (feat)
- [ ] Bug 修复 (fix)
- [ ] 重构 (refactor)
- [ ] 文档 (docs)
- [ ] 测试 (test)

## 测试计划
- [ ] 单元测试通过
- [ ] 集成测试通过
- [ ] 手动测试完成

## 相关 Issue
Closes #<issue-number>

## Checklist
- [ ] 代码符合项目规范
- [ ] 添加了必要的测试
- [ ] 更新了相关文档
- [ ] 无 clippy 警告
```

### 代码审查要求

1. **必须通过 CI 检查**
   - `cargo fmt --check`
   - `cargo clippy -- -D warnings`
   - `cargo test`

2. **至少 1 人 Review 批准**

3. **无未解决的评论**

4. **分支是最新的**（相对于 main）

## 合并策略

| 场景 | 策略 | 说明 |
|------|------|------|
| 功能分支 → main | **Squash and Merge** | 压缩为单个 commit |
| 紧急修复 | Merge Commit | 保留完整历史 |
| 发布分支 | Merge Commit | 保留发布历史 |

### Squash Commit 消息

```
feat(router): add IPv6 dual-stack support (#42)

* feat(router): add IPv6 route table management
* feat(dns): support AAAA record resolution
* test(router): add IPv6 route tests
```

## 版本发布流程

### 创建发布

```bash
# 1. 创建发布分支
git checkout -b release/v0.2.0

# 2. 更新版本号
# - Cargo.toml
# - CHANGELOG.md

# 3. 提交版本更新
git commit -m "chore(release): prepare v0.2.0"

# 4. 创建 PR 并合并到 main

# 5. 创建 Git Tag
git tag -a v0.2.0 -m "Release v0.2.0"
git push origin v0.2.0
```

### CHANGELOG 格式

```markdown
# Changelog

## [0.2.0] - 2026-03-27

### Added
- IPv6 dual-stack support for routing tables
- DNS AAAA record resolution support
- Config file hot-reload functionality

### Fixed
- Netlink socket reconnection handling
- DNS cache invalidation on network change

### Changed
- Refactored rule matching for better performance
- Improved error messages for config validation

### Breaking Changes
- Configuration format change (see migration guide)

## [0.1.0] - 2026-03-01

### Added
- Initial release
- Basic network interface monitoring
- Route table management
- CLI tool for status and control
```

## 禁止操作

| 操作 | 原因 |
|------|------|
| `git push --force` | 会覆盖他人提交 |
| 直接提交到 `main` | 绕过代码审查 |
| `git commit --amend` 已推送的提交 | 修改公共历史 |
| 删除已发布的 tag | 可能影响依赖此版本的用户 |

## 紧急修复流程

```bash
# 1. 从 main 创建修复分支
git checkout main
git checkout -b fix/critical-security-issue

# 2. 修复并测试
git commit -m "fix(security): patch CVE-2026-XXXX"
git push origin fix/critical-security-issue

# 3. 创建 PR（标记为紧急）
# 4. 快速审查后合并
# 5. 发布补丁版本
```
