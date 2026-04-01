# 代码审查清单

本文档定义 nic-autoswitch 项目的代码审查标准和检查项。

## 必查项清单

### 代码质量

- [ ] **编译无警告**
  ```bash
  cargo clippy -- -D warnings
  ```

- [ ] **格式正确**
  ```bash
  cargo fmt --check
  ```

- [ ] **测试通过**
  ```bash
  cargo test
  cargo test --all-features
  ```

- [ ] **文档完整**
  - 公共 API 都有文档注释
  - 文档示例可编译运行

- [ ] **无 unwrap/expect**（除非有充分理由）
  - 使用 `?` 操作符
  - 使用 `ok_or`/`ok_or_else`
  - expect 必须说明原因

### 错误处理

- [ ] **错误信息有上下文**
  ```rust
  // ✅ 好
  .map_err(|e| NicAutoSwitchError::Dns {
      domain: domain.to_string(),
      reason: e.to_string(),
  })?

  // ❌ 避免
  .map_err(|e| NicAutoSwitchError::Dns(e.to_string()))?
  ```

- [ ] **错误类型正确**
  - 库代码使用 `thiserror`
  - 应用代码可使用 `anyhow`

- [ ] **错误可恢复性评估**
  - 区分致命错误和可恢复错误
  - 实现适当的重试机制

### 性能考虑

- [ ] **无不必要的 clone/copy**
  ```rust
  // ✅ 好：使用引用
  fn process(data: &str) { }

  // ❌ 避免：不必要的克隆
  fn process(data: String) {
      let cloned = data.clone();  // 如果不需要，删除
  }
  ```

- [ ] **正确选择集合类型**
  - 频繁查找：`HashMap`/`HashSet`
  - 有序遍历：`BTreeMap`/`Vec`
  - 固定大小：数组

- [ ] **避免阻塞异步运行时**
  - 使用 `tokio::fs` 而非 `std::fs`
  - CPU 密集操作使用 `spawn_blocking`

### 安全检查

- [ ] **无硬编码凭据**
  ```rust
  // ❌ 绝对禁止
  const PASSWORD: &str = "secret123";

  // ✅ 从环境变量或配置读取
  let password = std::env::var("PASSWORD")?;
  ```

- [ ] **权限最小化**
  - 仅请求必要的 capabilities
  - 文件权限设置正确

- [ ] **输入验证**
  - 配置文件验证
  - CLI 参数验证
  - 网络输入验证

## 模块特定检查

### config 模块

- [ ] 配置结构有默认值
- [ ] 配置验证逻辑完整
- [ ] 配置文件示例与结构同步
- [ ] 热重载逻辑正确

### monitor 模块

- [ ] 网络事件正确处理
- [ ] 错误情况下优雅降级
- [ ] 状态同步正确

### router 模块

- [ ] 路由规则应用正确
- [ ] IPv4/IPv6 双栈处理
- [ ] DNS 缓存正确管理

### engine 模块

- [ ] 规则匹配逻辑正确
- [ ] 优先级处理正确
- [ ] 事件分发完整

### daemon 模块

- [ ] 信号处理正确
- [ ] Graceful shutdown 实现
- [ ] systemd 集成正确

### cli 模块

- [ ] 命令行参数验证
- [ ] 错误消息友好
- [ ] 帮助文档完整

## 代码风格检查

### 命名

- [ ] 变量/函数使用 snake_case
- [ ] 类型使用 PascalCase
- [ ] 常量使用 SCREAMING_SNAKE_CASE
- [ ] 名称有描述性

### 注释

- [ ] 公共 API 有文档注释
- [ ] 复杂逻辑有解释
- [ ] 注释与代码同步

### 结构

- [ ] 函数不超过 50 行
- [ ] 文件不超过 500 行
- [ ] 模块职责单一

## 测试检查

- [ ] **单元测试覆盖**
  - 正常路径
  - 边界条件
  - 错误情况

- [ ] **集成测试场景**
  - 配置加载
  - 路由应用
  - 事件处理

- [ ] **测试命名清晰**
  ```rust
  #[test]
  fn test_match_rule_with_cidr_returns_true() { }
  ```

## 异步代码检查

- [ ] 使用正确的锁类型
  - 跨 await 点使用 `tokio::sync::Mutex`
  - 短期持有可用 `std::sync::Mutex`

- [ ] 避免阻塞操作
  - 使用异步 I/O
  - CPU 密集用 `spawn_blocking`

- [ ] 超时设置合理
  - 网络操作有超时
  - 总体操作有超时

- [ ] 取消处理正确
  - CancellationToken 使用
  - select! 中处理关闭

## 文档检查

- [ ] README 更新（如有需要）
- [ ] CHANGELOG 更新
- [ ] 配置示例更新
- [ ] API 文档完整

## 依赖检查

- [ ] 新依赖必要
- [ ] 依赖版本合理
- [ ] 无安全漏洞
- [ ] features 最小化

## 审查流程

### 审查者步骤

1. 阅读描述理解变更目的
2. 检查 CI 是否通过
3. 按清单逐项检查
4. 提出建设性意见
5. 确认问题已解决

### 审查结果

| 结果 | 条件 |
|------|------|
| ✅ 批准 | 所有必查项通过 |
| ⚠️ 需修改 | 有小问题需修正 |
| ❌ 拒绝 | 有严重问题 |

## 常见问题示例

### 需要修改的代码

```rust
// 问题：错误信息不足
pub fn load(path: &Path) -> Result<Config> {
    let content = fs::read_to_string(path)?;  // 错误时不知道是哪个文件
}

// 修改建议
pub fn load(path: &Path) -> Result<Config> {
    let content = fs::read_to_string(path)
        .with_context(|| format!("Failed to read config: {}", path.display()))?;
}
```

```rust
// 问题：潜在的 panic
pub fn get(&self, key: &str) -> &Value {
    self.map.get(key).unwrap()  // 可能 panic
}

// 修改建议
pub fn get(&self, key: &str) -> Option<&Value> {
    self.map.get(key)
}
```

```rust
// 问题：阻塞异步运行时
pub async fn resolve(&self, domain: &str) -> Result<Vec<IpAddr>> {
    let addr = std::net::ToSocketAddrs::to_socket_addrs(&format!("{}:0", domain))?;
    // ...
}

// 修改建议
pub async fn resolve(&self, domain: &str) -> Result<Vec<IpAddr>> {
    self.resolver.lookup_ip(domain).await?
}
```
