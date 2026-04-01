# 性能基准

本文档定义 nic-autoswitch 项目的性能目标和基准测试规范。

## 关键性能指标

| 操作 | 目标延迟 | 最低要求 | 测量方法 |
|------|----------|----------|----------|
| 配置加载 | < 10ms | < 50ms | 从文件读取到解析完成 |
| 路由规则应用（单个） | < 1ms | < 10ms | 从调用到 netlink 响应 |
| 路由规则批量应用（100条） | < 50ms | < 200ms | 批量操作完成时间 |
| DNS 解析（缓存命中） | < 1ms | < 5ms | 从查询到返回结果 |
| DNS 解析（缓存未命中） | < 100ms | < 500ms | 实际网络查询 |
| 事件处理延迟 | < 100ms | < 500ms | 从事件产生到处理完成 |
| 规则匹配（100条规则） | < 1ms | < 5ms | 目标地址匹配时间 |
| CLI 响应 | < 100ms | < 500ms | 从命令发送到响应显示 |

## 内存使用

| 组件 | 目标内存 | 最大内存 |
|------|----------|----------|
| 守护进程（空闲） | < 5MB | < 10MB |
| 守护进程（活跃） | < 20MB | < 50MB |
| CLI 工具 | < 2MB | < 5MB |

## 基准测试配置

### Cargo.toml 配置

```toml
[dev-dependencies]
criterion = { version = "0.5", features = ["async_tokio"] }

[[bench]]
name = "matcher_bench"
harness = false

[[bench]]
name = "router_bench"
harness = false
```

### 基准测试目录结构

```
benches/
├── matcher_bench.rs      # 规则匹配性能测试
├── router_bench.rs       # 路由操作性能测试
└── dns_bench.rs          # DNS 解析性能测试
```

## 基准测试示例

### 规则匹配基准测试

```rust
// benches/matcher_bench.rs
use criterion::{black_box, criterion_group, criterion_main, Criterion, BenchmarkId};
use nic_autoswitch::engine::matcher::RuleMatcher;
use nic_autoswitch::config::{RouteRule, MatchOn};

fn create_matcher_with_rules(count: usize) -> RuleMatcher {
    let rules: Vec<RouteRule> = (0..count)
        .map(|i| RouteRule {
            name: format!("rule-{}", i),
            match_on: MatchOn::Cidr(format!("10.{}.0.0/16", i).parse().unwrap()),
            route_via: "eth0".to_string(),
            priority: i as u32 * 10,
        })
        .collect();

    RuleMatcher::new(rules)
}

fn rule_matching_benchmark(c: &mut Criterion) {
    let mut group = c.benchmark_group("rule_matching");

    for size in [10, 50, 100, 500, 1000].iter() {
        let matcher = create_matcher_with_rules(*size);
        let target = "10.50.100.200".parse().unwrap();

        group.bench_with_input(BenchmarkId::new("rules", size), &matcher, |b, matcher| {
            b.iter(|| matcher.match_destination(black_box(&target)))
        });
    }

    group.finish();
}

criterion_group!(benches, rule_matching_benchmark);
criterion_main!(benches);
```

### DNS 缓存基准测试

```rust
// benches/dns_bench.rs
use criterion::{black_box, criterion_group, criterion_main, Criterion};
use nic_autoswitch::router::dns::DnsResolver;

fn dns_cache_benchmark(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let resolver = rt.block_on(async { DnsResolver::new() });

    c.bench_function("dns_cache_hit", |b| {
        b.to_async(&rt).iter(|| async {
            // 第一次查询会缓存
            resolver.resolve("cached.example.com").await.unwrap();

            // 后续查询命中缓存
            resolver.resolve(black_box("cached.example.com")).await
        })
    });
}

criterion_group!(benches, dns_cache_benchmark);
criterion_main!(benches);
```

### 异步操作基准测试

```rust
// benches/router_bench.rs
use criterion::{black_box, criterion_group, criterion_main, Criterion, BatchSize};
use nic_autoswitch::router::Router;

fn route_apply_benchmark(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();

    c.bench_function("apply_single_route", |b| {
        b.iter_batched(
            || {
                // Setup: 创建路由规则
                create_test_route()
            },
            |route| {
                // Measure: 应用路由
                rt.block_on(async {
                    let router = Router::new().await.unwrap();
                    router.apply_route(black_box(&route)).await
                })
            },
            BatchSize::SmallInput,
        )
    });
}

criterion_group!(benches, route_apply_benchmark);
criterion_main!(benches);
```

## 运行基准测试

```bash
# 运行所有基准测试
cargo bench

# 运行特定基准测试
cargo bench --bench matcher_bench

# 保存基准结果用于比较
cargo bench -- --save-baseline main

# 与基准比较
cargo bench -- --baseline main

# 生成 HTML 报告
cargo bench -- --save-baseline new
# 报告位于 target/criterion/
```

## 性能分析工具

### 编译时间分析

```bash
# 查看编译时间
cargo build --timings

# 打开报告
open target/cargo-timings/cargo-timing.html
```

### 二进制大小分析

```bash
# 安装 cargo-bloat
cargo install cargo-bloat

# 分析 release 构建
cargo bloat --release

# 按 crate 分组
cargo bloat --release --crates

# 分析特定部分
cargo bloat --release --filter 'nic_autoswitch'
```

### 火焰图

```bash
# 安装 cargo-flamegraph
cargo install flamegraph

# 生成火焰图
cargo flamegraph --root -- nic-autoswitch --config config.toml

# 或 perf 采样
perf record -g ./target/release/nic-autoswitch
perf script | stackcollapse-perf.pl | flamegraph.pl > flamegraph.svg
```

### 内存分析

```bash
# 使用 valgrind
valgrind --tool=massif ./target/release/nic-autoswitch

# 使用 heaptrack
heaptrack ./target/release/nic-autoswitch
```

## 性能优化指南

### 常见优化点

1. **减少克隆**
   ```rust
   // 使用引用代替克隆
   fn process(data: &str) { }
   ```

2. **使用合适的数据结构**
   ```rust
   // 频繁查找用 HashMap
   let rules: HashMap<String, Rule> = ...;
   ```

3. **避免不必要的分配**
   ```rust
   // 预分配容量
   let mut v = Vec::with_capacity(expected_size);
   ```

4. **使用 Cow 避免分配**
   ```rust
   use std::borrow::Cow;

   fn get_name(&self) -> Cow<'_, str> {
       if self.name.is_empty() {
           Cow::Borrowed("default")
       } else {
           Cow::Borrowed(&self.name)
       }
   }
   ```

5. **延迟计算**
   ```rust
   // 只在需要时计算
   use once_cell::sync::Lazy;
   static CONFIG: Lazy<Config> = Lazy::new(|| load_config());
   ```

### 异步优化

1. **避免阻塞**
   ```rust
   // 使用 tokio 异步 API
   let content = tokio::fs::read_to_string(&path).await?;
   ```

2. **并行化独立操作**
   ```rust
   let (a, b) = tokio::join!(op_a(), op_b());
   ```

3. **使用 spawn 并发**
   ```rust
   let handles: Vec<_> = items.iter()
       .map(|item| tokio::spawn(process(item)))
       .collect();

   for handle in handles {
       handle.await??;
   }
   ```

## CI 性能监控

```yaml
# .github/workflows/bench.yml
name: Benchmarks
on: [pull_request]

jobs:
  bench:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable

      - name: Run benchmarks
        run: cargo bench -- --output-format bencher | tee bench_results.txt

      - name: Compare with main
        run: |
          # 下载 main 分支的基准结果
          # 比较并报告性能回归
```

## 性能回归阈值

| 指标 | 回归阈值 |
|------|----------|
| 延迟 | > 20% 下降需要解释 |
| 内存 | > 30% 增加需要解释 |
| 二进制大小 | > 10% 增加需要解释 |
| 编译时间 | > 15% 增加需要解释 |
