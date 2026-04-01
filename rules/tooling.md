# 工具配置

本文档定义 nic-autoswitch 项目的开发工具配置，包括 pre-commit hooks 和 CI/CD。

## Pre-commit Hooks

### 安装

```bash
# 安装 pre-commit
pip install pre-commit

# 或使用系统包管理器
# Arch: pacman -S pre-commit
# Ubuntu: apt install pre-commit

# 在项目中安装 hooks
pre-commit install
pre-commit install --hook-type commit-msg
```

### 配置文件

创建 `.pre-commit-config.yaml`：

```yaml
# .pre-commit-config.yaml
repos:
  # Rust 格式化
  - repo: local
    hooks:
      - id: cargo-fmt
        name: cargo fmt
        entry: cargo fmt --
        language: system
        types: [rust]
        pass_filenames: false

      # Clippy 检查
      - id: cargo-clippy
        name: cargo clippy
        entry: cargo clippy --all-targets --all-features -- -D warnings
        language: system
        types: [rust]
        pass_filenames: false

      # 单元测试
      - id: cargo-test
        name: cargo test
        entry: cargo test --lib
        language: system
        types: [rust]
        pass_filenames: false

      # 文档检查
      - id: cargo-doc
        name: cargo doc
        entry: bash -c 'RUSTDOCFLAGS="-D warnings" cargo doc --no-deps'
        language: system
        types: [rust]
        pass_filenames: false

  # 通用检查
  - repo: https://github.com/pre-commit/pre-commit-hooks
    rev: v4.5.0
    hooks:
      - id: trailing-whitespace
      - id: end-of-file-fixer
      - id: check-yaml
        args: [--unsafe]
      - id: check-toml
      - id: check-merge-conflict
      - id: check-added-large-files
        args: ['--maxkb=500']

  # Commit 消息格式
  - repo: https://github.com/commitizen-tools/commitizen
    rev: v3.13.0
    hooks:
      - id: commitizen
        stages: [commit-msg]

# CI 配置
ci:
  autofix_prs: false
  skip: [cargo-test]  # CI 中跳过耗时检查
```

### 手动运行

```bash
# 运行所有 hooks
pre-commit run --all-files

# 运行特定 hook
pre-commit run cargo-fmt --all-files

# 更新 hooks
pre-commit autoupdate
```

## GitHub Actions CI

### 主 CI 配置

创建 `.github/workflows/ci.yml`：

```yaml
# .github/workflows/ci.yml
name: CI

on:
  push:
    branches: [main]
  pull_request:
    branches: [main]

env:
  CARGO_TERM_COLOR: always
  RUST_BACKTRACE: 1

jobs:
  # 格式和 lint 检查
  lint:
    name: Lint
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4

      - name: Install Rust
        uses: dtolnay/rust-toolchain@stable
        with:
          components: rustfmt, clippy

      - name: Check formatting
        run: cargo fmt --check

      - name: Run clippy
        run: cargo clippy --all-targets --all-features -- -D warnings

      - name: Check documentation
        run: RUSTDOCFLAGS="-D warnings" cargo doc --no-deps

  # 单元测试
  test:
    name: Test
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4

      - name: Install Rust
        uses: dtolnay/rust-toolchain@stable

      - name: Cache cargo registry
        uses: actions/cache@v4
        with:
          path: |
            ~/.cargo/registry
            ~/.cargo/git
            target
          key: ${{ runner.os }}-cargo-${{ hashFiles('**/Cargo.lock') }}
          restore-keys: |
            ${{ runner.os }}-cargo-

      - name: Run tests
        run: cargo test --all-features

  # 测试覆盖率
  coverage:
    name: Coverage
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4

      - name: Install Rust
        uses: dtolnay/rust-toolchain@stable

      - name: Install tarpaulin
        run: cargo install cargo-tarpaulin

      - name: Generate coverage
        run: cargo tarpaulin --out Xml --output-dir coverage/

      - name: Upload to Codecov
        uses: codecov/codecov-action@v4
        with:
          files: coverage/cobertura.xml
          fail_ci_if_error: true
          token: ${{ secrets.CODECOV_TOKEN }}

  # 安全审计
  security:
    name: Security Audit
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4

      - name: Install Rust
        uses: dtolnay/rust-toolchain@stable

      - name: Install cargo-audit
        run: cargo install cargo-audit

      - name: Run security audit
        run: cargo audit

  # 构建检查
  build:
    name: Build
    runs-on: ubuntu-latest
    strategy:
      matrix:
        target:
          - x86_64-unknown-linux-gnu
          - x86_64-unknown-linux-musl  # 静态链接
    steps:
      - uses: actions/checkout@v4

      - name: Install Rust
        uses: dtolnay/rust-toolchain@stable
        with:
          targets: ${{ matrix.target }}

      - name: Build
        run: cargo build --release --target ${{ matrix.target }}

      - name: Upload artifact
        uses: actions/upload-artifact@v4
        with:
          name: binary-${{ matrix.target }}
          path: target/${{ matrix.target }}/release/nic-autoswitch
```

### 发布工作流

创建 `.github/workflows/release.yml`：

```yaml
# .github/workflows/release.yml
name: Release

on:
  push:
    tags:
      - 'v*'

permissions:
  contents: write

jobs:
  build:
    name: Build Release
    runs-on: ubuntu-latest
    strategy:
      matrix:
        target:
          - x86_64-unknown-linux-gnu
          - x86_64-unknown-linux-musl
    steps:
      - uses: actions/checkout@v4

      - name: Install Rust
        uses: dtolnay/rust-toolchain@stable
        with:
          targets: ${{ matrix.target }}

      - name: Build
        run: cargo build --release --target ${{ matrix.target }}

      - name: Package
        run: |
          cd target/${{ matrix.target }}/release
          tar -czvf nic-autoswitch-${{ matrix.target }}.tar.gz nic-autoswitch nic-autoswitch-cli

      - name: Upload artifact
        uses: actions/upload-artifact@v4
        with:
          name: release-${{ matrix.target }}
          path: target/${{ matrix.target }}/release/*.tar.gz

  release:
    name: Create Release
    needs: build
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4

      - name: Download artifacts
        uses: actions/download-artifact@v4
        with:
          pattern: release-*
          merge-multiple: true

      - name: Get version
        id: version
        run: echo "VERSION=${GITHUB_REF#refs/tags/}" >> $GITHUB_OUTPUT

      - name: Create Release
        uses: softprops/action-gh-release@v1
        with:
          name: Release ${{ steps.version.outputs.VERSION }}
          files: |
            *.tar.gz
          body_path: CHANGELOG.md
        env:
          GITHUB_TOKEN: ${{ secrets.GITHUB_TOKEN }}
```

### 依赖更新（Dependabot）

创建 `.github/dependabot.yml`：

```yaml
# .github/dependabot.yml
version: 2
updates:
  # Rust 依赖
  - package-ecosystem: cargo
    directory: /
    schedule:
      interval: weekly
      day: monday
      time: "06:00"
    open-pull-requests-limit: 5
    labels:
      - dependencies
      - rust
    commit-message:
      prefix: chore(deps)

  # GitHub Actions
  - package-ecosystem: github-actions
    directory: /
    schedule:
      interval: weekly
      day: monday
      time: "06:00"
    open-pull-requests-limit: 3
    labels:
      - dependencies
      - github-actions
    commit-message:
      prefix: chore(ci)
```

## 本地开发工具

### justfile（任务运行器）

```just
# justfile

# 默认：显示帮助
default:
    @just --list

# 格式化代码
fmt:
    cargo fmt

# 运行 clippy
clippy:
    cargo clippy --all-targets --all-features -- -D warnings

# 运行测试
test:
    cargo test --all-features

# 运行覆盖率
coverage:
    cargo tarpaulin --out Html

# 完整检查
check: fmt clippy test
    @echo "All checks passed!"

# 运行开发服务器
run:
    cargo run -- --config config.example.toml

# 构建 release
build:
    cargo build --release

# 安装 pre-commit hooks
install-hooks:
    pre-commit install
    pre-commit install --hook-type commit-msg

# 更新依赖
update-deps:
    cargo update
    cargo outdated
```

### rust-toolchain.toml

```toml
# rust-toolchain.toml
[toolchain]
channel = "stable"
components = ["rustfmt", "clippy", "rust-analyzer"]
profile = "default"
```

### .cargo/config.toml

```toml
# .cargo/config.toml

[alias]
xtask = "run --package xtask --"

[build]
# 增量编译
incremental = true

[term]
# 彩色输出
color = "auto"

# 并行编译
[target.x86_64-unknown-linux-gnu]
linker = "gcc"

[target.x86_64-unknown-linux-musl]
linker = "musl-gcc"
```

## 工具使用命令

```bash
# 运行 pre-commit
pre-commit run --all-files

# 运行 CI（本地模拟）
act  # 需要 act 工具

# 查看依赖更新
cargo outdated

# 安全审计
cargo audit

# 格式检查
cargo fmt --check

# Lint 检查
cargo clippy -- -D warnings

# 测试
cargo test

# 覆盖率
cargo tarpaulin --out Html

# 文档
cargo doc --open
```
