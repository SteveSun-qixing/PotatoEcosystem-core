# 贡献指南

感谢您对 Chips-core 项目的关注！我们欢迎各种形式的贡献。

## 贡献方式

### 报告问题

如果您发现了 bug 或有功能建议：

1. 首先搜索现有的 Issues，避免重复
2. 使用 Issue 模板创建新 Issue
3. 提供详细的描述和复现步骤

### 提交代码

1. Fork 项目仓库
2. 创建功能分支：`git checkout -b feature/your-feature`
3. 编写代码和测试
4. 确保所有检查通过
5. 提交 Pull Request

## 代码规范

### Rust 代码规范

- 使用 `cargo fmt` 格式化代码
- 使用 `cargo clippy` 检查代码质量
- 所有公开 API 必须有文档注释
- 遵循 Rust API Guidelines

### 提交信息规范

使用 Conventional Commits 格式：

```
<type>(<scope>): <subject>

<body>

<footer>
```

**类型 (type)**:
- `feat`: 新功能
- `fix`: 修复 bug
- `docs`: 文档更新
- `style`: 代码格式（不影响功能）
- `refactor`: 重构
- `test`: 测试相关
- `chore`: 构建/工具相关

**示例**:
```
feat(router): 添加路由缓存优化

- 实现 LRU 缓存
- 添加缓存命中率统计
- 支持缓存失效机制

Closes #123
```

## 测试要求

提交代码前，请确保：

1. **所有测试通过**
   ```bash
   cargo test
   ```

2. **代码质量检查通过**
   ```bash
   cargo clippy -- -D warnings
   ```

3. **代码格式正确**
   ```bash
   cargo fmt -- --check
   ```

4. **新功能有测试覆盖**
   - 单元测试
   - 集成测试（如适用）

## Pull Request 流程

1. **创建 PR**
   - 使用清晰的标题
   - 填写 PR 描述模板
   - 关联相关 Issue

2. **代码审查**
   - 至少需要一个审查批准
   - 回应审查意见

3. **合并**
   - 所有检查通过后合并
   - 使用 Squash and Merge

## 开发环境

### 依赖

- Rust 1.70+
- Tokio 1.35+

### 构建

```bash
# 开发构建
cargo build

# 发布构建
cargo build --release

# 运行测试
cargo test

# 运行基准测试
cargo bench

# 生成文档
cargo doc --open
```

## 项目结构

```
Chips-core/
├── src/
│   ├── api/          # SDK 和 IPC 接口
│   ├── core/         # 核心配置
│   ├── module/       # 模块管理系统
│   ├── router/       # 路由系统
│   └── utils/        # 工具函数
├── tests/            # 集成测试
├── benches/          # 性能基准测试
└── examples/         # 使用示例
```

## 行为准则

请尊重所有贡献者，保持友好和专业的交流环境。

## 许可证

通过贡献代码，您同意您的贡献将按照 MIT 许可证进行授权。

---

如有问题，请随时在 Issues 中提问！
