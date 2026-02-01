# 12-API完整索引

## 1. 核心API

### 1.1 内核初始化API

#### `Core::initialize`

**函数签名**
```rust
pub async fn initialize(config: CoreConfig) -> Result<Self>
```

**参数**
- `config: CoreConfig` - 内核配置对象

**返回值**
- `Result<Self>` - 初始化的内核实例或错误

**错误码**
- `INVALID_CONFIG` - 配置无效
- `IO_ERROR` - 文件系统错误
- `MODULE_LOAD_FAILED` - 模块加载失败

**示例**
```rust
let config = CoreConfig {
    module_dirs: vec![PathBuf::from("./modules")],
    config_path: Some(PathBuf::from("./config.yaml")),
    log_level: LogLevel::Info,
    debug: false,
};

let core = ChipsCore::initialize(config).await?;
```

---

#### `Core::start`

**函数签名**
```rust
pub async fn start(&self) -> Result<()>
```

**返回值**
- `Result<()>` - 成功或错误

**错误码**
- `INVALID_STATE` - 内核状态无效
- `INTERNAL_ERROR` - 内部错误

**示例**
```rust
core.start().await?;
```

---

#### `Core::stop`

**函数签名**
```rust
pub async fn stop(&self) -> Result<()>
```

**返回值**
- `Result<()>` - 成功或错误

**示例**
```rust
core.stop().await?;
```

---

## 2. 路由API

### 2.1 路由请求API

#### `Router::route`

**函数签名**
```rust
pub async fn route(&self, request: RouteRequest) -> RouteResponse
```

**参数**
- `request: RouteRequest` - 路由请求对象
  - `request_id: String` - 请求ID（可选，默认生成UUID）
  - `sender: String` - 发送者模块ID
  - `action: String` - 操作名称
  - `target: Option<String>` - 目标模块ID（可选）
  - `params: HashMap<String, Value>` - 参数
  - `priority: i32` - 优先级（默认0）
  - `timeout_ms: u64` - 超时时间（默认30000ms）

**返回值**
- `RouteResponse` - 路由响应对象
  - `request_id: String` - 请求ID
  - `status: i32` - 状态码（200成功，400+客户端错误，500+服务器错误）
  - `data: Option<Value>` - 响应数据
  - `error: Option<ErrorInfo>` - 错误信息
  - `elapsed_ms: u64` - 耗时（毫秒）

**示例**
```rust
let request = RouteRequest {
    sender: "app".to_string(),
    action: "file.open".to_string(),
    target: None,
    params: {
        let mut map = HashMap::new();
        map.insert("file_id".to_string(), json!("abc123"));
        map
    },
    priority: 0,
    timeout_ms: 5000,
    ..Default::default()
};

let response = router.route(request).await;

if response.status == 200 {
    println!("Success: {:?}", response.data);
} else {
    eprintln!("Error: {:?}", response.error);
}
```

---

#### `Router::route_batch`

**函数签名**
```rust
pub async fn route_batch(&self, requests: Vec<RouteRequest>) -> Vec<RouteResponse>
```

**参数**
- `requests: Vec<RouteRequest>` - 路由请求列表

**返回值**
- `Vec<RouteResponse>` - 路由响应列表

**示例**
```rust
let requests = vec![
    RouteRequest { action: "action1".to_string(), ..Default::default() },
    RouteRequest { action: "action2".to_string(), ..Default::default() },
];

let responses = router.route_batch(requests).await;
```

---

### 2.2 事件订阅API

#### `Router::subscribe`

**函数签名**
```rust
pub async fn subscribe(&self, event_type: &str, subscriber: &str) -> Result<()>
```

**参数**
- `event_type: &str` - 事件类型
- `subscriber: &str` - 订阅者模块ID

**返回值**
- `Result<()>` - 成功或错误

**示例**
```rust
router.subscribe("file.imported", "my-module").await?;
```

---

#### `Router::unsubscribe`

**函数签名**
```rust
pub async fn unsubscribe(&self, event_type: &str, subscriber: &str) -> Result<()>
```

**参数**
- `event_type: &str` - 事件类型
- `subscriber: &str` - 订阅者模块ID

**返回值**
- `Result<()>` - 成功或错误

**示例**
```rust
router.unsubscribe("file.imported", "my-module").await?;
```

---

#### `Router::broadcast`

**函数签名**
```rust
pub async fn broadcast(&self, event: Event)
```

**参数**
- `event: Event` - 事件对象
  - `event_id: String` - 事件ID
  - `event_type: String` - 事件类型
  - `sender: String` - 发送者模块ID
  - `data: Value` - 事件数据
  - `timestamp: String` - 时间戳（ISO 8601格式）

**示例**
```rust
let event = Event {
    event_id: Uuid::new_v4().to_string(),
    event_type: "file.imported".to_string(),
    sender: "my-module".to_string(),
    data: json!({ "file_id": "abc123" }),
    timestamp: Utc::now().to_rfc3339(),
};

router.broadcast(event).await;
```

---

### 2.3 路由注册API

#### `Router::register_route`

**函数签名**
```rust
pub async fn register_route(&self, action: &str, module_id: &str, priority: i32) -> Result<()>
```

**参数**
- `action: &str` - 操作名称
- `module_id: &str` - 处理模块ID
- `priority: i32` - 优先级（数值越大优先级越高）

**返回值**
- `Result<()>` - 成功或错误

**示例**
```rust
router.register_route("file.open", "editor-module", 100).await?;
```

---

#### `Router::unregister_route`

**函数签名**
```rust
pub async fn unregister_route(&self, action: &str, module_id: &str) -> Result<()>
```

**参数**
- `action: &str` - 操作名称
- `module_id: &str` - 模块ID

**返回值**
- `Result<()>` - 成功或错误

**示例**
```rust
router.unregister_route("file.open", "editor-module").await?;
```

---

## 3. 模块管理API

### 3.1 模块扫描API

#### `ModuleManager::scan`

**函数签名**
```rust
pub async fn scan(&self) -> Result<Vec<String>>
```

**返回值**
- `Result<Vec<String>>` - 发现的模块ID列表或错误

**示例**
```rust
let module_ids = module_manager.scan().await?;
println!("Found {} modules", module_ids.len());
```

---

### 3.2 模块注册API

#### `ModuleManager::register`

**函数签名**
```rust
pub async fn register(&self, module_path: &Path) -> Result<String>
```

**参数**
- `module_path: &Path` - 模块路径

**返回值**
- `Result<String>` - 模块ID或错误

**错误码**
- `INVALID_MODULE` - 模块无效
- `MODULE_ALREADY_EXISTS` - 模块已存在

**示例**
```rust
let module_id = module_manager
    .register(Path::new("./modules/my-module"))
    .await?;
```

---

### 3.3 模块加载API

#### `ModuleManager::load`

**函数签名**
```rust
pub async fn load(&self, module_id: &str) -> Result<()>
```

**参数**
- `module_id: &str` - 模块ID

**返回值**
- `Result<()>` - 成功或错误

**错误码**
- `MODULE_NOT_FOUND` - 模块不存在
- `MODULE_LOAD_FAILED` - 加载失败
- `DEPENDENCY_NOT_FOUND` - 依赖不存在
- `VERSION_MISMATCH` - 版本不兼容
- `CIRCULAR_DEPENDENCY` - 循环依赖

**示例**
```rust
module_manager.load("my-module").await?;
```

---

#### `ModuleManager::unload`

**函数签名**
```rust
pub async fn unload(&self, module_id: &str) -> Result<()>
```

**参数**
- `module_id: &str` - 模块ID

**返回值**
- `Result<()>` - 成功或错误

**错误码**
- `MODULE_NOT_FOUND` - 模块不存在
- `MODULE_HAS_DEPENDENTS` - 有其他模块依赖此模块

**示例**
```rust
module_manager.unload("my-module").await?;
```

---

### 3.4 模块查询API

#### `ModuleManager::get_module`

**函数签名**
```rust
pub async fn get_module(&self, module_id: &str) -> Option<ModuleInfo>
```

**参数**
- `module_id: &str` - 模块ID

**返回值**
- `Option<ModuleInfo>` - 模块信息或None
  - `id: String` - 模块ID
  - `name: String` - 模块名称
  - `version: String` - 版本
  - `description: Option<String>` - 描述
  - `author: Option<String>` - 作者
  - `dependencies: Vec<Dependency>` - 依赖列表
  - `state: ModuleState` - 状态

**示例**
```rust
if let Some(info) = module_manager.get_module("my-module").await {
    println!("Module: {} v{}", info.name, info.version);
}
```

---

#### `ModuleManager::list_modules`

**函数签名**
```rust
pub async fn list_modules(&self) -> Vec<ModuleInfo>
```

**返回值**
- `Vec<ModuleInfo>` - 所有模块信息列表

**示例**
```rust
let modules = module_manager.list_modules().await;
for module in modules {
    println!("{}: {}", module.id, module.state);
}
```

---

#### `ModuleManager::get_state`

**函数签名**
```rust
pub async fn get_state(&self, module_id: &str) -> Option<ModuleState>
```

**参数**
- `module_id: &str` - 模块ID

**返回值**
- `Option<ModuleState>` - 模块状态或None
  - `Unloaded` - 未加载
  - `Loading` - 加载中
  - `Running` - 运行中
  - `Error` - 错误状态

**示例**
```rust
if let Some(state) = module_manager.get_state("my-module").await {
    println!("State: {:?}", state);
}
```

---

## 4. 文件识别API

### 4.1 文件类型识别API

#### `FileRecognizer::recognize`

**函数签名**
```rust
pub async fn recognize(&self, file_path: &Path) -> Result<FileType>
```

**参数**
- `file_path: &Path` - 文件路径

**返回值**
- `Result<FileType>` - 文件类型或错误
  - `ChipsCard` - 卡片文件
  - `ChipsBox` - 箱子文件
  - `Image(ImageFormat)` - 图片文件
  - `Video(VideoFormat)` - 视频文件
  - `Audio(AudioFormat)` - 音频文件
  - `Document(DocFormat)` - 文档文件
  - `Archive` - 压缩文件
  - `Unknown` - 未知类型

**示例**
```rust
let file_type = recognizer.recognize(Path::new("test.cchips")).await?;

match file_type {
    FileType::ChipsCard => println!("This is a card file"),
    FileType::Image(fmt) => println!("Image: {:?}", fmt),
    _ => println!("Other type"),
}
```

---

#### `FileRecognizer::recognize_by_extension`

**函数签名**
```rust
pub fn recognize_by_extension(&self, file_path: &Path) -> Option<FileType>
```

**参数**
- `file_path: &Path` - 文件路径

**返回值**
- `Option<FileType>` - 文件类型或None

**示例**
```rust
let file_type = recognizer.recognize_by_extension(Path::new("test.cchips"));
```

---

#### `FileRecognizer::recognize_by_magic`

**函数签名**
```rust
pub async fn recognize_by_magic(&self, file_path: &Path) -> Result<Option<FileType>>
```

**参数**
- `file_path: &Path` - 文件路径

**返回值**
- `Result<Option<FileType>>` - 文件类型或错误

**示例**
```rust
let file_type = recognizer.recognize_by_magic(Path::new("test.dat")).await?;
```

---

### 4.2 卡片文件解析API

#### `FileRecognizer::parse_card`

**函数签名**
```rust
pub async fn parse_card(&self, file_path: &Path) -> Result<CardFile>
```

**参数**
- `file_path: &Path` - 卡片文件路径

**返回值**
- `Result<CardFile>` - 卡片文件对象或错误
  - `metadata: CardMetadata` - 元数据
  - `content: CardContent` - 内容
  - `resources: Vec<Resource>` - 资源列表

**错误码**
- `INVALID_FILE_FORMAT` - 文件格式无效
- `FILE_CORRUPTED` - 文件损坏
- `UNSUPPORTED_VERSION` - 不支持的版本

**示例**
```rust
let card = recognizer.parse_card(Path::new("todo.cchips")).await?;
println!("Card type: {}", card.metadata.card_type);
```

---

## 5. 资源管理API

### 5.1 资源读取API

#### `ResourceManager::read`

**函数签名**
```rust
pub async fn read(&self, resource_path: &str) -> Result<Vec<u8>>
```

**参数**
- `resource_path: &str` - 资源路径
  - 本地文件: `file:///path/to/file` 或 `./path/to/file`
  - HTTP资源: `https://example.com/resource`
  - WebDAV资源: `webdav://server/path`

**返回值**
- `Result<Vec<u8>>` - 资源数据或错误

**错误码**
- `RESOURCE_NOT_FOUND` - 资源不存在
- `RESOURCE_ACCESS_DENIED` - 访问被拒绝
- `RESOURCE_TOO_LARGE` - 资源过大
- `NETWORK_ERROR` - 网络错误

**示例**
```rust
// 读取本地文件
let data = resource_manager.read("file:///path/to/file.txt").await?;

// 读取HTTP资源
let data = resource_manager.read("https://example.com/image.png").await?;
```

---

#### `ResourceManager::read_to_string`

**函数签名**
```rust
pub async fn read_to_string(&self, resource_path: &str) -> Result<String>
```

**参数**
- `resource_path: &str` - 资源路径

**返回值**
- `Result<String>` - 字符串内容或错误

**示例**
```rust
let content = resource_manager.read_to_string("./config.yaml").await?;
```

---

### 5.2 资源写入API

#### `ResourceManager::write`

**函数签名**
```rust
pub async fn write(&self, resource_path: &str, data: &[u8]) -> Result<()>
```

**参数**
- `resource_path: &str` - 资源路径
- `data: &[u8]` - 数据

**返回值**
- `Result<()>` - 成功或错误

**示例**
```rust
let data = b"Hello, World!";
resource_manager.write("./output.txt", data).await?;
```

---

### 5.3 资源元信息API

#### `ResourceManager::exists`

**函数签名**
```rust
pub async fn exists(&self, resource_path: &str) -> Result<bool>
```

**参数**
- `resource_path: &str` - 资源路径

**返回值**
- `Result<bool>` - 是否存在或错误

**示例**
```rust
if resource_manager.exists("./file.txt").await? {
    println!("File exists");
}
```

---

#### `ResourceManager::metadata`

**函数签名**
```rust
pub async fn metadata(&self, resource_path: &str) -> Result<ResourceMetadata>
```

**参数**
- `resource_path: &str` - 资源路径

**返回值**
- `Result<ResourceMetadata>` - 元信息或错误
  - `size: u64` - 大小（字节）
  - `modified: Option<DateTime<Utc>>` - 修改时间
  - `mime_type: Option<String>` - MIME类型
  - `is_directory: bool` - 是否目录

**示例**
```rust
let meta = resource_manager.metadata("./file.txt").await?;
println!("Size: {} bytes", meta.size);
```

---

### 5.4 资源操作API

#### `ResourceManager::delete`

**函数签名**
```rust
pub async fn delete(&self, resource_path: &str) -> Result<()>
```

**参数**
- `resource_path: &str` - 资源路径

**返回值**
- `Result<()>` - 成功或错误

**示例**
```rust
resource_manager.delete("./old-file.txt").await?;
```

---

#### `ResourceManager::copy`

**函数签名**
```rust
pub async fn copy(&self, from: &str, to: &str) -> Result<()>
```

**参数**
- `from: &str` - 源路径
- `to: &str` - 目标路径

**返回值**
- `Result<()>` - 成功或错误

**示例**
```rust
resource_manager.copy("./source.txt", "./dest.txt").await?;
```

---

#### `ResourceManager::move_resource`

**函数签名**
```rust
pub async fn move_resource(&self, from: &str, to: &str) -> Result<()>
```

**参数**
- `from: &str` - 源路径
- `to: &str` - 目标路径

**返回值**
- `Result<()>` - 成功或错误

**示例**
```rust
resource_manager.move_resource("./old.txt", "./new.txt").await?;
```

---

## 6. 配置API

### 6.1 配置读取API

#### `ConfigManager::get`

**函数签名**
```rust
pub fn get<T: DeserializeOwned>(&self, key: &str) -> Option<T>
```

**参数**
- `key: &str` - 配置键（支持点分隔符，如 `"server.port"`）

**返回值**
- `Option<T>` - 配置值或None

**示例**
```rust
let port: u16 = config_manager.get("server.port").unwrap_or(8080);
let debug: bool = config_manager.get("debug").unwrap_or(false);
```

---

#### `ConfigManager::set`

**函数签名**
```rust
pub fn set<T: Serialize>(&mut self, key: &str, value: T) -> Result<()>
```

**参数**
- `key: &str` - 配置键
- `value: T` - 配置值

**返回值**
- `Result<()>` - 成功或错误

**示例**
```rust
config_manager.set("server.port", 9000)?;
```

---

## 7. 日志API

### 7.1 日志记录API

#### 日志宏

**宏签名**
```rust
debug!(message, args...);
info!(message, args...);
warn!(message, args...);
error!(message, args...);
```

**示例**
```rust
use tracing::{debug, info, warn, error};

debug!("Debug message");
info!("Processing file: {}", file_id);
warn!("Low memory: {} MB", available_mb);
error!("Failed to load module: {}", error);

// 结构化日志
info!(
    request_id = %req_id,
    module = "router",
    elapsed_ms = elapsed,
    "Request completed"
);
```

---

## 8. 错误码完整列表

### 8.1 通用错误码

| 错误码 | 数值 | 说明 |
|--------|------|------|
| `OK` | 200 | 成功 |
| `BAD_REQUEST` | 400 | 请求错误 |
| `UNAUTHORIZED` | 401 | 未授权 |
| `FORBIDDEN` | 403 | 禁止访问 |
| `NOT_FOUND` | 404 | 未找到 |
| `TIMEOUT` | 408 | 超时 |
| `INTERNAL_ERROR` | 500 | 内部错误 |
| `NOT_IMPLEMENTED` | 501 | 未实现 |
| `SERVICE_UNAVAILABLE` | 503 | 服务不可用 |

### 8.2 模块相关错误码

| 错误码 | 说明 |
|--------|------|
| `MODULE_NOT_FOUND` | 模块不存在 |
| `MODULE_LOAD_FAILED` | 模块加载失败 |
| `MODULE_ALREADY_LOADED` | 模块已加载 |
| `DEPENDENCY_NOT_FOUND` | 依赖不存在 |
| `VERSION_MISMATCH` | 版本不匹配 |
| `CIRCULAR_DEPENDENCY` | 循环依赖 |

### 8.3 路由相关错误码

| 错误码 | 说明 |
|--------|------|
| `NO_ROUTE_FOUND` | 未找到路由 |
| `TARGET_MODULE_NOT_RUNNING` | 目标模块未运行 |

### 8.4 资源相关错误码

| 错误码 | 说明 |
|--------|------|
| `RESOURCE_NOT_FOUND` | 资源不存在 |
| `RESOURCE_ACCESS_DENIED` | 访问被拒绝 |
| `RESOURCE_TOO_LARGE` | 资源过大 |

### 8.5 文件相关错误码

| 错误码 | 说明 |
|--------|------|
| `INVALID_FILE_FORMAT` | 文件格式无效 |
| `FILE_CORRUPTED` | 文件损坏 |
| `NO_HANDLER_FOUND` | 未找到处理器 |

---

## 9. API版本和兼容性

**当前API版本**: `1.0.0`

**版本策略**
- 主版本号：不兼容的API变更
- 次版本号：向后兼容的功能新增
- 修订号：向后兼容的问题修正

**废弃策略**
- 废弃的API在至少一个次版本中保留
- 使用时发出警告
- 在下一个主版本中移除

---

## 10. 完整示例

### 10.1 基础使用示例

```rust
use chips_core::*;

#[tokio::main]
async fn main() -> Result<()> {
    // 1. 初始化内核
    let config = CoreConfig {
        module_dirs: vec![PathBuf::from("./modules")],
        config_path: Some(PathBuf::from("./config.yaml")),
        log_level: LogLevel::Info,
        debug: false,
    };
    
    let core = ChipsCore::initialize(config).await?;
    core.start().await?;
    
    // 2. 获取组件
    let router = core.router();
    let module_manager = core.module_manager();
    let resource_manager = core.resource_manager();
    
    // 3. 加载模块
    module_manager.load("editor-module").await?;
    
    // 4. 发送路由请求
    let request = RouteRequest {
        sender: "app".to_string(),
        action: "file.open".to_string(),
        params: {
            let mut map = HashMap::new();
            map.insert("file_id".to_string(), json!("abc123"));
            map
        },
        ..Default::default()
    };
    
    let response = router.route(request).await;
    
    if response.status == 200 {
        println!("Success!");
    }
    
    // 5. 停止内核
    core.stop().await?;
    
    Ok(())
}
```

### 10.2 模块开发示例

```rust
use chips_core::*;

pub struct MyModule {
    context: ModuleContext,
}

impl Module for MyModule {
    async fn on_init(&mut self) -> Result<()> {
        // 订阅事件
        self.context.router.subscribe("file.imported", &self.context.module_id).await?;
        
        // 注册路由
        self.context.router.register_route(
            "my.custom.action",
            &self.context.module_id,
            100
        ).await?;
        
        Ok(())
    }
    
    async fn on_start(&mut self) -> Result<()> {
        self.context.logger.info("Module started");
        Ok(())
    }
    
    async fn handle_request(&mut self, request: RouteRequest) -> RouteResponse {
        match request.action.as_str() {
            "my.custom.action" => {
                // 处理请求
                RouteResponse::success(&request.request_id, json!({ "result": "ok" }), 0)
            }
            _ => RouteResponse::error(
                &request.request_id,
                404,
                ErrorInfo::new("NO_ROUTE_FOUND", "Unknown action"),
                0
            ),
        }
    }
}
```

---

完整的API索引为模块开发提供了清晰的参考,所有API都包含详细的参数、返回值、错误码和示例。
