//! Benchmark tests for router operations
//!
//! Run with: cargo bench --bench router_bench

use criterion::{Criterion, black_box, criterion_group, criterion_main};
use nic_autoswitch::config::{MatchOn, RouteRule, RouteVia};
use std::sync::Arc;

use nic_autoswitch::router::{DnsResolver, RouteManager, RuleOperator};

fn create_test_rule(name: &str, priority: u32) -> RouteRule {
    RouteRule {
        name: name.to_string(),
        match_on: MatchOn::Cidr {
            cidr: "10.0.0.0/8".parse().unwrap(),
        },
        route_via: RouteVia {
            interface: "eth0".to_string(),
        },
        priority,
    }
}

fn bench_route_manager_creation(c: &mut Criterion) {
    c.bench_function("route_manager_new", |b| {
        b.iter(|| RouteManager::new(black_box(100)))
    });

    c.bench_function("route_manager_default", |b| b.iter(RouteManager::default));
}

fn bench_table_id_calculation(c: &mut Criterion) {
    let manager = RouteManager::default();

    c.bench_function("table_id_v4_calculation", |b| {
        b.iter(|| manager.table_id_v4(black_box(5)))
    });

    c.bench_function("table_id_v6_calculation", |b| {
        b.iter(|| manager.table_id_v6(black_box(5)))
    });
}

fn bench_rule_validation(c: &mut Criterion) {
    let manager = RouteManager::default();
    let operator = RuleOperator::new(Arc::new(manager));
    let rule = create_test_rule("test-rule", 100);

    c.bench_function("validate_rule", |b| {
        b.iter(|| operator.validate_rule(black_box(&rule)))
    });
}

fn bench_rule_application_stub(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();

    c.bench_function("apply_rule_stub", |b| {
        b.to_async(&rt).iter(|| async {
            let manager = RouteManager::default();
            let operator = RuleOperator::new(Arc::new(manager));
            let rule = create_test_rule("test-rule", 100);
            operator.apply_rule(&rule, 0, 100).await.unwrap()
        })
    });
}

fn bench_rule_removal_stub(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();

    c.bench_function("remove_rule_stub", |b| {
        b.to_async(&rt).iter(|| async {
            let manager = RouteManager::default();
            let operator = RuleOperator::new(Arc::new(manager));
            let rule = create_test_rule("test-rule", 100);
            operator.remove_rule(&rule, 100).await.unwrap()
        })
    });
}

fn bench_dns_resolver(c: &mut Criterion) {
    let resolver = DnsResolver::default();

    c.bench_function("dns_resolver_default", |b| b.iter(DnsResolver::default));

    c.bench_function("dns_cache_prune", |b| b.iter(|| resolver.prune_cache()));

    c.bench_function("dns_cache_clear", |b| b.iter(|| resolver.clear_cache()));
}

fn bench_cidr_parsing(c: &mut Criterion) {
    c.bench_function("parse_ipv4_cidr", |b| {
        b.iter(|| "10.0.0.0/8".parse::<ipnetwork::IpNetwork>())
    });

    c.bench_function("parse_ipv6_cidr", |b| {
        b.iter(|| "2001:db8::/32".parse::<ipnetwork::IpNetwork>())
    });

    c.bench_function("parse_ipv4_address", |b| {
        b.iter(|| "192.168.1.1".parse::<std::net::IpAddr>())
    });

    c.bench_function("parse_ipv6_address", |b| {
        b.iter(|| "2001:db8::1".parse::<std::net::IpAddr>())
    });
}

criterion_group!(
    benches,
    bench_route_manager_creation,
    bench_table_id_calculation,
    bench_rule_validation,
    bench_rule_application_stub,
    bench_rule_removal_stub,
    bench_dns_resolver,
    bench_cidr_parsing,
);
criterion_main!(benches);
