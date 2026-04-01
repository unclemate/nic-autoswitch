//! Benchmark tests for rule matching
//!
//! Run with: cargo bench --bench matcher_bench

use criterion::{Criterion, black_box, criterion_group, criterion_main};
use nic_autoswitch::config::{MatchOn, RouteRule, RouteVia};
use nic_autoswitch::engine::{Destination, RuleMatcher};
use std::net::IpAddr;

fn create_benchmark_rules() -> Vec<RouteRule> {
    (0..100)
        .map(|i| RouteRule {
            name: format!("rule-{}", i),
            match_on: if i % 4 == 0 {
                MatchOn::Cidr {
                    cidr: format!("10.{}.0.0/16", i).parse().unwrap(),
                }
            } else if i % 4 == 1 {
                MatchOn::Ip {
                    ip: format!("192.168.{}.{}", i / 256, i % 256).parse().unwrap(),
                }
            } else if i % 4 == 2 {
                MatchOn::Domain {
                    domain: format!("service{}.example.com", i),
                }
            } else {
                MatchOn::DomainPattern {
                    domain_pattern: format!("*.domain{}.example.com", i),
                }
            },
            route_via: RouteVia {
                interface: if i % 2 == 0 { "eth0" } else { "wlan0" }.to_string(),
            },
            priority: i as u32 * 10,
        })
        .collect()
}

fn bench_matcher_creation(c: &mut Criterion) {
    c.bench_function("rule_matcher_new", |b| b.iter(RuleMatcher::new));
}

fn bench_destination_creation(c: &mut Criterion) {
    c.bench_function("destination_ip", |b| {
        b.iter(|| Destination::ip(black_box("10.5.0.1".parse::<IpAddr>().unwrap())))
    });

    c.bench_function("destination_domain", |b| {
        b.iter(|| Destination::domain(black_box("example.com")))
    });
}

fn bench_direct_matching(c: &mut Criterion) {
    let matcher = RuleMatcher::new();
    let rules = create_benchmark_rules();

    // Benchmark IP destination matching
    let rt = tokio::runtime::Runtime::new().unwrap();

    c.bench_function("match_ip_destination", |b| {
        b.to_async(&rt).iter(|| {
            let matcher = &matcher;
            let rules = &rules;
            let dest = Destination::ip("10.5.0.1".parse().unwrap());
            async move {
                matcher
                    .find_matching_rule(black_box(&dest), black_box(rules))
                    .await
                    .unwrap()
            }
        })
    });

    c.bench_function("match_domain_destination", |b| {
        b.to_async(&rt).iter(|| {
            let matcher = &matcher;
            let rules = &rules;
            let dest = Destination::domain("service50.example.com");
            async move {
                matcher
                    .find_matching_rule(black_box(&dest), black_box(rules))
                    .await
                    .unwrap()
            }
        })
    });

    c.bench_function("match_no_match", |b| {
        b.to_async(&rt).iter(|| {
            let matcher = &matcher;
            let rules = &rules;
            let dest = Destination::ip("203.0.113.1".parse().unwrap());
            async move {
                matcher
                    .find_matching_rule(black_box(&dest), black_box(rules))
                    .await
                    .unwrap()
            }
        })
    });
}

fn bench_match_on_construction(c: &mut Criterion) {
    c.bench_function("match_on_cidr", |b| {
        b.iter(|| MatchOn::Cidr {
            cidr: "10.0.0.0/8".parse().unwrap(),
        })
    });

    c.bench_function("match_on_ip", |b| {
        b.iter(|| MatchOn::Ip {
            ip: "192.168.1.1".parse().unwrap(),
        })
    });

    c.bench_function("match_on_domain", |b| {
        b.iter(|| MatchOn::Domain {
            domain: "example.com".to_string(),
        })
    });

    c.bench_function("match_on_domain_pattern", |b| {
        b.iter(|| MatchOn::DomainPattern {
            domain_pattern: "*.example.com".to_string(),
        })
    });
}

criterion_group!(
    benches,
    bench_matcher_creation,
    bench_destination_creation,
    bench_direct_matching,
    bench_match_on_construction,
);
criterion_main!(benches);
