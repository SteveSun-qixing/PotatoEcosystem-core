# Chips-core 与 SDK 接口对接规范

**版本**: 1.0.0  
**更新日期**: 2026-02-01  
**状态**: 正式版

---

## 1. 概述

本文档定义 Chips-core（薯片微内核）与 Chips-SDK 之间的接口对接规范，确保两个项目能够完美配合工作。

### 1.1 架构关系

```
┌─────────────────────────────────────────────────────────┐
│                    Chips SDK (TypeScript)               │
│  ┌─────────────────────────────────────────────────┐   │
│  │              CoreConnector                       │   │
│  │  - connect()                                     │   │
│  │  - request()                                     │   │
│  │  - disconnect()                                  │   │
│  └─────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────┘
                            │
                     IPC 通信层
                     (JSON over Socket)
                            │
┌─────────────────────────────────────────────────────────┐
│                Chips Core (Rust)                        │
│  ┌─────────────────────────────────────────────────┐   │
│  │              IpcServer                           │   │
│  │  - 接收请求                                       │   │
│  │  - 路由分发                                       │   │
│  │  - 返回响应                                       │   │
│  └─────────────────────────────────────────────────┘   │
│                            │                            │
│  ┌──────────┬──────────┬──────────┬──────────────┐     │
│  │ Router   │ EventBus │ Config   │ ModuleManager│     │
│  └──────────┴──────────┴──────────┴──────────────┘     │
└─────────────────────────────────────────────────────────┘
```

---

## 2. 通信协议

### 2.1 传输方式

Chips-core 支持两种传输方式：

| 方式 | 平台 | 默认地址 |
|------|------|----------|
| Unix Socket | Unix/Linux/macOS | `/tmp/chips-core.sock` |
| TCP Socket | 所有平台 | `127.0.0.1:9527` |

### 2.2 消息格式

使用 **NDJSON**（换行分隔的 JSON）格式：

```
{"id":"req-001","message_type":"Route","payload":{...},"timestamp":"2026-02-01T12:00:00Z"}\n
```

---

## 3. 请求/响应格式

### 3.1 IPC 请求 (IpcRequest)

```json
{
  "id": "req-001",
  "message_type": "Route",
  "payload": {
    "sender": "sdk-client",
    "action": "resource.read",
    "params": { "path": "/path/to/card.card" }
  },
  "timestamp": "2026-02-01T12:00:00Z"
}
```

### 3.2 IPC 响应 (IpcResponse)

**成功响应:**
```json
{
  "request_id": "req-001",
  "success": true,
  "data": { "result": "..." },
  "timestamp": "2026-02-01T12:00:01Z"
}
```

**错误响应:**
```json
{
  "request_id": "req-001",
  "success": false,
  "error": "File not found",
  "timestamp": "2026-02-01T12:00:01Z"
}
```

---

## 4. 消息类型

### 4.1 支持的消息类型

| 类型 | 说明 | SDK 对应方法 |
|------|------|-------------|
| `Route` | 路由请求 | `coreConnector.request()` |
| `Publish` | 发布事件 | `eventBus.emit()` |
| `Subscribe` | 订阅事件 | `eventBus.on()` |
| `Unsubscribe` | 取消订阅 | `eventBus.off()` |
| `ConfigGet` | 读取配置 | `configManager.get()` |
| `ConfigSet` | 设置配置 | `configManager.set()` |
| `Status` | 获取状态 | `sdk.getStatus()` |
| `Heartbeat` | 心跳检测 | 自动处理 |

### 4.2 路由请求 (Route)

**请求 payload:**
```json
{
  "sender": "client-id",
  "action": "service.method",
  "params": { ... },
  "timeout_ms": 30000
}
```

**action 格式:** `service.method`
- `resource.read` - 读取资源
- `resource.write` - 写入资源
- `module.load` - 加载模块
- `module.unload` - 卸载模块
- 其他由 Foundation 模块注册的 action

### 4.3 事件发布 (Publish)

**请求 payload:**
```json
{
  "event_type": "card:load",
  "sender": "client-id",
  "data": { ... }
}
```

### 4.4 事件订阅 (Subscribe)

**请求 payload:**
```json
{
  "subscriber_id": "client-id",
  "event_type": "card:*",
  "filter": null
}
```

**响应 data:**
```json
{
  "subscription_id": "sub-001"
}
```

### 4.5 配置操作 (ConfigGet/ConfigSet)

**ConfigGet payload:**
```json
{
  "key": "router.worker_count"
}
```

**ConfigSet payload:**
```json
{
  "key": "router.worker_count",
  "value": 8
}
```

---

## 5. SDK CoreConnector 实现参考

### 5.1 TypeScript 示例

```typescript
class CoreConnector {
  private socket: WebSocket | null = null;
  private pendingRequests = new Map<string, { resolve, reject }>();

  async connect(url: string): Promise<void> {
    // 连接到 Chips-core IPC 服务
    this.socket = new WebSocket(url);
    
    this.socket.onmessage = (event) => {
      const response = JSON.parse(event.data);
      const pending = this.pendingRequests.get(response.request_id);
      if (pending) {
        if (response.success) {
          pending.resolve(response.data);
        } else {
          pending.reject(new Error(response.error));
        }
        this.pendingRequests.delete(response.request_id);
      }
    };
  }

  async request(params: RequestParams): Promise<any> {
    const id = generateUuid();
    const request: IpcRequest = {
      id,
      message_type: 'Route',
      payload: {
        sender: 'sdk-client',
        action: `${params.service}.${params.method}`,
        params: params.payload,
        timeout_ms: params.timeout || 30000,
      },
      timestamp: new Date().toISOString(),
    };

    return new Promise((resolve, reject) => {
      this.pendingRequests.set(id, { resolve, reject });
      this.socket!.send(JSON.stringify(request) + '\n');
      
      // 超时处理
      setTimeout(() => {
        if (this.pendingRequests.has(id)) {
          this.pendingRequests.delete(id);
          reject(new Error('Request timeout'));
        }
      }, params.timeout || 30000);
    });
  }
}
```

---

## 6. 事件系统对接

### 6.1 事件类型映射

| SDK 事件 | Core 事件 |
|----------|-----------|
| `card:load` | `card.loaded` |
| `card:save` | `card.saved` |
| `box:load` | `box.loaded` |
| `sdk:ready` | `core.started` |
| `theme:change` | `theme.changed` |

### 6.2 事件推送

当 Core 有事件需要推送给 SDK 时，使用以下格式：

```json
{
  "type": "event",
  "event_type": "card.loaded",
  "sender": "chips-core",
  "data": { "card_id": "..." },
  "timestamp": "2026-02-01T12:00:00Z"
}
```

---

## 7. 错误处理

### 7.1 错误码

| 错误码 | 说明 |
|--------|------|
| `ROUTE_NOT_FOUND` | 路由未找到 |
| `ROUTE_TIMEOUT` | 路由超时 |
| `MODULE_NOT_FOUND` | 模块未找到 |
| `PERMISSION_DENIED` | 权限不足 |
| `INVALID_REQUEST` | 请求格式无效 |
| `INTERNAL_ERROR` | 内部错误 |

### 7.2 SDK 错误处理建议

```typescript
class ChipsError extends Error {
  constructor(
    public code: string,
    message: string,
    public details?: any
  ) {
    super(message);
    this.name = 'ChipsError';
  }
}

// 使用示例
try {
  const result = await coreConnector.request({ ... });
} catch (error) {
  if (error.code === 'ROUTE_NOT_FOUND') {
    // 处理路由未找到
  }
}
```

---

## 8. 版本兼容性

### 8.1 协议版本

当前协议版本: `1.0.0`

请求中可包含协议版本：
```json
{
  "protocol_version": "1.0.0",
  ...
}
```

### 8.2 兼容性规则

- 主版本不同：不兼容
- 次版本不同：向后兼容
- 补丁版本不同：完全兼容

---

## 9. 性能建议

1. **连接复用**: SDK 应保持长连接，避免频繁重连
2. **批量请求**: 对于批量操作，使用批量 API 减少往返次数
3. **超时设置**: 根据操作类型设置合理的超时时间
4. **心跳维护**: 启用心跳检测，及时发现连接断开

---

## 10. 测试建议

### 10.1 连接测试
- 测试 Unix Socket 连接（Unix 平台）
- 测试 TCP 连接（所有平台）
- 测试连接超时处理
- 测试连接断开重连

### 10.2 功能测试
- 测试所有消息类型
- 测试错误处理
- 测试并发请求
- 测试大数据传输

---

**文档维护者**: Chips 生态核心团队
