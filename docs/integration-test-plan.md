# 集成测试计划

## 1. 概述

本文档定义 nic-autoswitch 项目的集成测试策略。所有集成测试在 Docker 容器中执行，通过 `CAP_NET_ADMIN` 能力和 dummy 网络接口模拟真实网络环境。

## 2. 测试环境

### 2.1 Docker 环境要求

| 项目 | 要求 |
|------|------|
| 基础镜像 | `rust:1.93-bookworm` |
| Linux 能力 | `CAP_NET_ADMIN`, `CAP_NET_RAW` |
| 网络模式 | `none`（隔离外部网络） |
| 系统工具 | `iproute2`, `net-tools` |

### 2.2 模拟网络拓扑

`scripts/setup-netns.sh` 创建以下虚拟接口：

```
nic0 (LAN)  — 192.168.1.100/24  — dummy 接口
nic1 (WLAN) — 192.168.2.100/24  — dummy 接口
nic2 (VPN)  — 10.8.0.1/24       — dummy 接口
```

### 2.3 执行方式

```bash
# 运行全部集成测试
docker compose run --rm test cargo test --test 'integration_*'

# 运行单个测试文件
docker compose run --rm test cargo test --test integration_route

# 交互式调试
docker compose run --rm dev
```

## 3. 测试文件结构

```
tests/
├── common/
│   ├── mod.rs                  # 共享工具函数
│   └── mock_network.rs         # mock 网络组件
├── fixtures/
│   ├── valid_config.toml       # 基础有效配置
│   ├── complex_config.toml     # 多 profile 多规则配置
│   ├── ipv6_config.toml        # IPv6 双栈配置
│   └── invalid/
│       ├── bad_toml.toml       # TOML 语法错误
│       ├── empty_interfaces.toml  # 无接口
│       └── invalid_table_id.toml  # 非法 table_id
├── integration_config.rs       # [已有] 配置加载测试
├── integration_engine.rs       # [已有] 引擎调度测试
├── integration_route.rs        # [新增] 路由管理测试（需 NET_ADMIN）
├── integration_daemon.rs       # [新增] 守护进程生命周期测试
├── integration_cli.rs          # [新增] CLI 通信测试
└── integration_e2e.rs          # [新增] 端到端场景测试
```

## 4. 测试场景与用例

### 4.1 配置管理（integration_config.rs）— 已有 + 补充

> 无需 Docker 特权，可在任意环境运行

| ID | 用例 | 场景 | 期望 |
|----|------|------|------|
| CFG-01 | 加载有效配置 | 标准 TOML 配置 | 解析成功，字段正确 |
| CFG-02 | 加载含 WiFi profile 配置 | 含 rules 的 profile | profile.rules 解析正确 |
| CFG-03 | 加载含默认规则配置 | `routing.default_rules` | default_rules 非空 |
| CFG-04 | 配置文件不存在 | 路径无效 | 返回含 "not found" 的错误 |
| CFG-05 | TOML 语法错误 | `invalid [[[` | 返回含 "Failed to parse" 的错误 |
| CFG-06 | 空接口列表 | 无 `[interfaces.*]` | 返回含 "At least one interface" 的错误 |
| CFG-07 | 非法 table_id_start | table_id_start = 50 | 返回含 "table_id_start" 的错误 |
| CFG-08 | 热重载配置 | 修改文件后 reload | monitor_interval 更新 |
| CFG-09 | IP 精确匹配规则 | `match_on = { ip = "..." }` | 解析成功 |
| CFG-10 | 域名匹配规则 | `match_on = { domain = "..." }` | 解析成功 |
| CFG-11 | **[新]** 通配符域名规则 | `match_on = { domain_pattern = "*.corp.com" }` | 解析成功 |
| CFG-12 | **[新]** 多 profile 配置 | 2+ WiFi profiles | 各 profile 独立 |
| CFG-13 | **[新]** VPN 类型接口 | `interface_type = "vpn"` | 解析成功 |
| CFG-14 | **[新]** MAC 地址匹配 | `match_by = { mac = "xx:xx:..." }` | 解析成功 |
| CFG-15 | **[新]** 重复规则名 | 两个同名 rule | 验证失败 |

### 4.2 引擎调度（integration_engine.rs）— 已有 + 补充

> 无需 Docker 特权

| ID | 用例 | 场景 | 期望 |
|----|------|------|------|
| ENG-01 | 初始化状态 | new + state() | `Initializing` |
| ENG-02 | 启停生命周期 | start → stop | `Running` → `Stopped` |
| ENG-03 | 未启动忽略事件 | Initializing 下发事件 | 不报错 |
| ENG-04 | WiFi 连接事件 | WifiConnected("CorpWiFi") | SSID 追踪正确 |
| ENG-05 | WiFi 断开事件 | Connected → Disconnected | SSID 清空 |
| ENG-06 | 未知 SSID 用默认规则 | 未知 SSID 连接 | 使用 default_rules |
| ENG-07 | 配置热更新 | update_config() | 无 panic |
| ENG-08 | 初始无活跃路由 | active_routes() | 空 |
| ENG-09 | 初始无网络状态 | network_state() | 空 |
| ENG-10 | **[新]** 重复 WiFi 连接 | 同 SSID 重复连接 | 幂等，不重复应用规则 |
| ENG-11 | **[新]** WiFi 切换 | CorpWiFi → HomeWiFi | 清除旧规则，应用新规则 |
| ENG-12 | **[新]** 地址变更事件 | AddressChanged | 状态正确更新 |
| ENG-13 | **[新]** 接口上线事件 | InterfaceChanged(Up) | 状态标记为 up |
| ENG-14 | **[新]** 接口下线事件 | InterfaceChanged(Down) | 状态标记为 down，清路由 |
| ENG-15 | **[新]** 并发事件处理 | 多事件快速下发 | 无竞争、无 panic |

### 4.3 路由管理（integration_route.rs）— 新增，需 Docker

> **需要 `CAP_NET_ADMIN`**，必须在 Docker 容器中运行

| ID | 用例 | 场景 | 期望 |
|----|------|------|------|
| RTE-01 | 添加 IPv4 路由 | `ip route add 10.0.0.0/8 dev nic0 table 100` | `ip route show table 100` 可见 |
| RTE-02 | 删除 IPv4 路由 | 添加后删除 | `ip route show table 100` 不可见 |
| RTE-03 | 添加策略路由规则 | `ip rule add to 10.0.0.0/8 table 100` | `ip rule show` 可见 |
| RTE-04 | 删除策略路由规则 | 添加后删除 | `ip rule show` 不可见 |
| RTE-05 | 冲突路由覆盖 | 同目标同 table 重复添加 | 后者覆盖，无报错 |
| RTE-06 | 多接口路由隔离 | nic0 → table100, nic1 → table101 | 路由表互不干扰 |
| RTE-07 | flush 接口路由 | 添加多条后 flush | 该 table 清空 |
| RTE-08 | 网关路由 | via 192.168.1.1 dev nic0 table 100 | 正确设置网关 |
| RTE-09 | 单 IP 转 CIDR | 192.168.1.1 → 192.168.1.1/32 | 路由正确添加 |
| RTE-10 | metric 优先级 | 同目标不同 metric | 高 metric 路由存在 |
| RTE-11 | **[新]** IPv6 路由添加 | `ip -6 route add fd00::/64 dev nic0 table 200` | IPv6 路由可见 |
| RTE-12 | **[新]** IPv6 策略规则 | IPv6 策略路由添加 | `ip -6 rule show` 可见 |
| RTE-13 | **[新]** 不存在的接口 | route_via 引用不存在的接口 | 返回错误 |
| RTE-14 | **[新]** 路由存在性检查 | route_exists() 查已有路由 | 返回 true |
| RTE-15 | **[新]** 删除不存在的路由 | 删除未添加的路由 | 不报错（幂等） |

### 4.4 DNS 解析（integration_dns.rs）— 新增

> 容器 network_mode=none，DNS 测试需 mock 或内网 DNS 服务

| ID | 用例 | 场景 | 期望 |
|----|------|------|------|
| DNS-01 | 缓存命中 | 同域名两次 resolve | 第二次用缓存 |
| DNS-02 | 缓存过期 | 超过 TTL 后 resolve | 重新查询 |
| DNS-03 | 缓存统计 | 添加若干条目 | stats 正确 |
| DNS-04 | 清除缓存 | clear_cache() | stats 归零 |
| DNS-05 | 过期清理 | prune_cache() | 仅移除过期条目 |
| DNS-06 | IPv4 专用解析 | resolve_ipv4() | 仅返回 IPv4 地址 |
| DNS-07 | IPv6 专用解析 | resolve_ipv6() | 仅返回 IPv6 地址 |
| DNS-08 | 并发查询 | 同域名多 task 并发 | 只发一次实际查询 |

### 4.5 守护进程生命周期（integration_daemon.rs）— 新增，需 Docker

| ID | 用例 | 场景 | 期望 |
|----|------|------|------|
| DMN-01 | 启动→运行 | service.run() 启动 | 状态 `Running` |
| DMN-02 | 优雅停止 | 发送 Shutdown 信号 | 状态 `Stopped` |
| DMN-03 | 信号重载 | 发送 Reload 信号 | 配置重新加载 |
| DMN-04 | 网络事件处理 | 模拟接口变化 | 路由规则更新 |
| DMN-05 | dry-run 模式 | dry_run=true | 不实际修改路由表 |
| DMN-06 | Unix Socket 启动 | control server start | socket 文件存在 |
| DMN-07 | Unix Socket 停止 | control server stop | socket 文件清理 |
| DMN-08 | 状态查询 | 通过 control 查状态 | 返回正确 DaemonStatus |
| DMN-09 | **[新]** 配置热加载 | 修改配置文件 | 自动应用新配置 |
| DMN-10 | **[新]** 多信号交替 | Reload × 3 → Shutdown | 无 panic，最终停止 |

### 4.6 CLI 通信（integration_cli.rs）— 新增，需 Docker

| ID | 用例 | 场景 | 期望 |
|----|------|------|------|
| CLI-01 | 守护进程未运行 | client.get_status() | 返回连接错误 |
| CLI-02 | 连接运行中的守护进程 | 启动 daemon → client 查询 | 返回正确状态 |
| CLI-03 | status 命令 | `nic-autoswitch-cli status` | 格式化输出 |
| CLI-04 | routes 命令 | `nic-autoswitch-cli routes` | 显示活跃路由 |
| CLI-05 | reload 命令 | `nic-autoswitch-cli reload` | 守护进程重载配置 |
| CLI-06 | shutdown 命令 | `nic-autoswitch-cli shutdown` | 守护进程停止 |
| CLI-07 | JSON 输出 | `--json` 参数 | 有效 JSON |
| CLI-08 | 超时处理 | 守护进程不响应 | 返回超时错误 |

### 4.7 端到端场景（integration_e2e.rs）— 新增，需 Docker

> 完整模拟真实使用场景

| ID | 用例 | 场景 | 期望 |
|----|------|------|------|
| E2E-01 | 公司 WiFi 连接 | 连接 CorpWiFi → 检查路由 | 10.0.0.0/8 走 nic0 |
| E2E-02 | WiFi 切换 | CorpWiFi → HomeWiFi | 旧规则清除，新规则生效 |
| E2E-03 | WiFi 断开 | 断开所有 WiFi | 回退到 default_rules |
| E2E-04 | 接口上线 | 接口 down → up | 路由重新应用 |
| E2E-05 | 配置变更 | 修改配置并热加载 | 路由规则更新 |
| E2E-06 | 多规则优先级 | 同一 profile 多条规则 | 按 priority 排序生效 |
| E2E-07 | **[新]** IPv6 双栈 | IPv6 地址事件 | IPv4/IPv6 路由独立管理 |
| E2E-08 | **[新]** dry-run 验证 | dry_run=true 全流程 | 路由表不变 |

## 5. Mock 策略

### 5.1 系统依赖分层

| 依赖 | 策略 | 说明 |
|------|------|------|
| rtnetlink | Docker + dummy 接口 | 真实 netlink 操作 |
| D-Bus/NetworkManager | Mock | 容器内无 NM，用 trait mock |
| DNS (hickory-resolver) | Mock resolver + 缓存测试 | 无网络环境用本地缓存测试 |
| 文件系统 | tempfile | 隔离配置文件 |
| Unix Socket | 容器内临时目录 | 真实 socket 通信 |
| 系统信号 | 内部 broadcast channel | 不依赖真实信号 |

### 5.2 Feature Flag

```toml
# Cargo.toml
[features]
default = []
test-integration = []   # 集成测试标记
test-mock-dns = []      # Mock DNS 解析
test-mock-netlink = []  # Mock netlink（CI 无特权环境使用）
```

## 6. Docker 集成测试配置

### 6.1 docker-compose.yml 新增服务

```yaml
  test-integration:
    build: .
    container_name: nic-inttest
    network_mode: none
    cap_add: [NET_ADMIN, NET_RAW]
    volumes:
      - ./config.example.toml:/etc/nic-autoswitch/config.toml
    entrypoint: ["bash", "scripts/setup-netns.sh", "--", "cargo", "test", "--test", "integration_route", "--test", "integration_daemon", "--test", "integration_cli", "--test", "integration_e2e", "--", "--test-threads=1"]
```

### 6.2 CI 集成

```yaml
  integration-test:
    name: Integration Tests
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - name: Build and run integration tests in Docker
        run: docker compose run --rm test-integration
```

## 7. 覆盖率目标

| 测试类型 | 目标文件 | 覆盖率要求 |
|----------|----------|------------|
| 配置管理 | config/*.rs | ≥ 85% |
| 网络监控 | monitor/*.rs | ≥ 70% |
| 路由管理 | router/*.rs | ≥ 80% |
| 引擎核心 | engine/*.rs | ≥ 85% |
| 守护进程 | daemon/*.rs | ≥ 75% |
| CLI | cli/*.rs | ≥ 70% |
| **关键路径** | matcher.rs, executor.rs, dispatcher.rs | **100%** |

## 8. 测试执行顺序

```
1. integration_config     ← 无特权，快速反馈
2. integration_engine     ← 无特权，快速反馈
3. integration_dns        ← 部分需要特权
4. integration_route      ← 需 Docker + NET_ADMIN
5. integration_daemon     ← 需 Docker + NET_ADMIN
6. integration_cli        ← 需 Docker + NET_ADMIN + daemon 运行
7. integration_e2e        ← 需 Docker + NET_ADMIN，全流程
```

## 9. 注意事项

1. **单线程执行**：集成测试必须 `--test-threads=1`，避免路由表竞争
2. **幂等性**：每个测试必须自行清理（删除添加的路由/规则）
3. **跳过策略**：无 `CAP_NET_ADMIN` 时，路由测试应 skip 而非 fail
4. **超时设置**：异步测试设置合理超时（默认 60s）
5. **日志调试**：集成测试开启 debug 日志，失败时输出到 stderr
