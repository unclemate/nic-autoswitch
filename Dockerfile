FROM rust:1.93-bookworm

RUN apt-get update && apt-get install -y \
    iproute2 iputils-ping net-tools ethtool \
    gdb strace lsof vim-nox procps \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /usr/src/nic-autoswitch

# 复制全部源码并完整编译（构建时有网络，可下载依赖）
COPY Cargo.toml Cargo.lock ./
COPY src/ src/
COPY benches/ benches/
COPY tests/ tests/
RUN cargo build && cargo test --no-run

# 配置和脚本
COPY config.example.toml /etc/nic-autoswitch/config.toml
COPY scripts/ scripts/

CMD ["/bin/bash"]
