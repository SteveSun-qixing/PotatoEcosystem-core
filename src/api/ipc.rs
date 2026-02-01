//! IPC 进程间通信接口
//!
//! 本模块实现薯片微内核的进程间通信（IPC）功能，支持：
//!
//! - Unix Socket（Unix/Linux/macOS）
//! - TCP Socket（跨平台备选方案）
//!
//! # 架构
//!
//! ```text
//! ┌─────────────┐         ┌─────────────┐
//! │  IpcClient  │◄───────►│  IpcServer  │
//! └─────────────┘         └──────┬──────┘
//!                                │
//!                         ┌──────▼──────┐
//!                         │  ChipsCore  │
//!                         └─────────────┘
//! ```
//!
//! # 消息协议
//!
//! IPC 使用 JSON 格式的消息协议：
//! - 每条消息以换行符（`\n`）结尾
//! - 支持请求/响应模式
//! - 支持事件推送
//!
//! # 示例
//!
//! ## 启动 IPC 服务器
//!
//! ```rust,no_run
//! use chips_core::api::ipc::{IpcServer, IpcConfig};
//! use chips_core::{ChipsCore, CoreConfig};
//! use std::sync::Arc;
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let core = Arc::new(ChipsCore::new(CoreConfig::default()).await?);
//!     let config = IpcConfig::unix_socket("/tmp/chips.sock");
//!     let server = IpcServer::new(config, core);
//!     server.start().await?;
//!     Ok(())
//! }
//! ```
//!
//! ## 使用 IPC 客户端
//!
//! ```rust,no_run
//! use chips_core::api::ipc::{IpcClient, IpcConfig, IpcRequest};
//! use serde_json::json;
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let config = IpcConfig::unix_socket("/tmp/chips.sock");
//!     let mut client = IpcClient::connect(config).await?;
//!     
//!     let request = IpcRequest::route("client", "test.action", json!({}));
//!     let response = client.send_request(request).await?;
//!     
//!     println!("Response: {:?}", response);
//!     Ok(())
//! }
//! ```

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::{broadcast, RwLock};
use tracing::{debug, error, info, warn};

use crate::router::{Event, RouteRequest};
use crate::utils::{generate_uuid, CoreError, Result};

// ============================================================================
// IPC 配置
// ============================================================================

/// IPC 传输类型
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IpcTransport {
    /// Unix Domain Socket（Unix/Linux/macOS）
    #[cfg(unix)]
    UnixSocket(PathBuf),
    /// TCP Socket（跨平台）
    Tcp(String, u16),
}

impl Default for IpcTransport {
    fn default() -> Self {
        #[cfg(unix)]
        {
            IpcTransport::UnixSocket(PathBuf::from("/tmp/chips-core.sock"))
        }
        #[cfg(not(unix))]
        {
            IpcTransport::Tcp("127.0.0.1".to_string(), 9527)
        }
    }
}

/// IPC 配置
#[derive(Debug, Clone)]
pub struct IpcConfig {
    /// 传输类型
    pub transport: IpcTransport,
    /// 最大连接数
    pub max_connections: usize,
    /// 连接超时（毫秒）
    pub connection_timeout_ms: u64,
    /// 请求超时（毫秒）
    pub request_timeout_ms: u64,
    /// 是否启用心跳
    pub enable_heartbeat: bool,
    /// 心跳间隔（秒）
    pub heartbeat_interval_secs: u64,
    /// 缓冲区大小
    pub buffer_size: usize,
}

impl Default for IpcConfig {
    fn default() -> Self {
        Self {
            transport: IpcTransport::default(),
            max_connections: 100,
            connection_timeout_ms: 5000,
            request_timeout_ms: 30000,
            enable_heartbeat: true,
            heartbeat_interval_secs: 30,
            buffer_size: 64 * 1024, // 64KB
        }
    }
}

impl IpcConfig {
    /// 创建 Unix Socket 配置
    #[cfg(unix)]
    pub fn unix_socket(path: impl Into<PathBuf>) -> Self {
        Self {
            transport: IpcTransport::UnixSocket(path.into()),
            ..Default::default()
        }
    }

    /// 创建 TCP 配置
    pub fn tcp(host: impl Into<String>, port: u16) -> Self {
        Self {
            transport: IpcTransport::Tcp(host.into(), port),
            ..Default::default()
        }
    }

    /// 设置最大连接数
    pub fn max_connections(mut self, max: usize) -> Self {
        self.max_connections = max;
        self
    }

    /// 设置连接超时
    pub fn connection_timeout(mut self, timeout_ms: u64) -> Self {
        self.connection_timeout_ms = timeout_ms;
        self
    }

    /// 设置请求超时
    pub fn request_timeout(mut self, timeout_ms: u64) -> Self {
        self.request_timeout_ms = timeout_ms;
        self
    }

    /// 禁用心跳
    pub fn disable_heartbeat(mut self) -> Self {
        self.enable_heartbeat = false;
        self
    }
}

// ============================================================================
// IPC 消息协议
// ============================================================================

/// IPC 消息类型
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IpcMessageType {
    /// 路由请求
    Route,
    /// 发布事件
    Publish,
    /// 订阅事件
    Subscribe,
    /// 取消订阅
    Unsubscribe,
    /// 心跳
    Heartbeat,
    /// 心跳响应
    HeartbeatAck,
    /// 配置读取
    ConfigGet,
    /// 配置设置
    ConfigSet,
    /// 获取状态
    Status,
    /// 关闭连接
    Disconnect,
}

/// IPC 请求
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IpcRequest {
    /// 请求 ID
    pub id: String,
    /// 消息类型
    pub message_type: IpcMessageType,
    /// 请求负载
    #[serde(default)]
    pub payload: Value,
    /// 时间戳
    pub timestamp: DateTime<Utc>,
}

impl IpcRequest {
    /// 创建新请求
    pub fn new(message_type: IpcMessageType, payload: Value) -> Self {
        Self {
            id: generate_uuid(),
            message_type,
            payload,
            timestamp: Utc::now(),
        }
    }

    /// 创建路由请求
    pub fn route(sender: impl Into<String>, action: impl Into<String>, params: Value) -> Self {
        Self::new(
            IpcMessageType::Route,
            serde_json::json!({
                "sender": sender.into(),
                "action": action.into(),
                "params": params
            }),
        )
    }

    /// 创建发布事件请求
    pub fn publish(event_type: impl Into<String>, sender: impl Into<String>, data: Value) -> Self {
        Self::new(
            IpcMessageType::Publish,
            serde_json::json!({
                "event_type": event_type.into(),
                "sender": sender.into(),
                "data": data
            }),
        )
    }

    /// 创建订阅请求
    pub fn subscribe(subscriber_id: impl Into<String>, event_type: impl Into<String>) -> Self {
        Self::new(
            IpcMessageType::Subscribe,
            serde_json::json!({
                "subscriber_id": subscriber_id.into(),
                "event_type": event_type.into()
            }),
        )
    }

    /// 创建取消订阅请求
    pub fn unsubscribe(subscription_id: impl Into<String>) -> Self {
        Self::new(
            IpcMessageType::Unsubscribe,
            serde_json::json!({
                "subscription_id": subscription_id.into()
            }),
        )
    }

    /// 创建心跳请求
    pub fn heartbeat() -> Self {
        Self::new(IpcMessageType::Heartbeat, Value::Null)
    }

    /// 创建配置读取请求
    pub fn config_get(key: impl Into<String>) -> Self {
        Self::new(
            IpcMessageType::ConfigGet,
            serde_json::json!({
                "key": key.into()
            }),
        )
    }

    /// 创建配置设置请求
    pub fn config_set(key: impl Into<String>, value: Value) -> Self {
        Self::new(
            IpcMessageType::ConfigSet,
            serde_json::json!({
                "key": key.into(),
                "value": value
            }),
        )
    }

    /// 创建状态请求
    pub fn status() -> Self {
        Self::new(IpcMessageType::Status, Value::Null)
    }

    /// 创建断开连接请求
    pub fn disconnect() -> Self {
        Self::new(IpcMessageType::Disconnect, Value::Null)
    }

    /// 序列化为 JSON 字符串（以换行符结尾）
    pub fn to_line(&self) -> Result<String> {
        let mut json = serde_json::to_string(self)?;
        json.push('\n');
        Ok(json)
    }

    /// 从 JSON 字符串解析
    pub fn from_line(line: &str) -> Result<Self> {
        let request: IpcRequest = serde_json::from_str(line.trim())?;
        Ok(request)
    }
}

/// IPC 响应
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IpcResponse {
    /// 对应的请求 ID
    pub request_id: String,
    /// 是否成功
    pub success: bool,
    /// 响应数据
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
    /// 错误信息
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// 时间戳
    pub timestamp: DateTime<Utc>,
}

impl IpcResponse {
    /// 创建成功响应
    pub fn success(request_id: impl Into<String>, data: Value) -> Self {
        Self {
            request_id: request_id.into(),
            success: true,
            data: Some(data),
            error: None,
            timestamp: Utc::now(),
        }
    }

    /// 创建成功响应（无数据）
    pub fn success_empty(request_id: impl Into<String>) -> Self {
        Self {
            request_id: request_id.into(),
            success: true,
            data: None,
            error: None,
            timestamp: Utc::now(),
        }
    }

    /// 创建错误响应
    pub fn error(request_id: impl Into<String>, error: impl Into<String>) -> Self {
        Self {
            request_id: request_id.into(),
            success: false,
            data: None,
            error: Some(error.into()),
            timestamp: Utc::now(),
        }
    }

    /// 序列化为 JSON 字符串（以换行符结尾）
    pub fn to_line(&self) -> Result<String> {
        let mut json = serde_json::to_string(self)?;
        json.push('\n');
        Ok(json)
    }

    /// 从 JSON 字符串解析
    pub fn from_line(line: &str) -> Result<Self> {
        let response: IpcResponse = serde_json::from_str(line.trim())?;
        Ok(response)
    }
}

/// IPC 事件通知（推送给客户端）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IpcEvent {
    /// 事件 ID
    pub event_id: String,
    /// 事件类型
    pub event_type: String,
    /// 发送方
    pub sender: String,
    /// 事件数据
    pub data: Value,
    /// 时间戳
    pub timestamp: DateTime<Utc>,
}

impl From<Event> for IpcEvent {
    fn from(event: Event) -> Self {
        Self {
            event_id: event.event_id,
            event_type: event.event_type,
            sender: event.sender,
            data: event.data,
            timestamp: event.timestamp,
        }
    }
}

// ============================================================================
// IPC 服务器
// ============================================================================

/// IPC 服务器统计
#[derive(Debug, Clone, Default)]
pub struct IpcServerStats {
    /// 总连接数
    pub total_connections: u64,
    /// 当前活跃连接数
    pub active_connections: u64,
    /// 总请求数
    pub total_requests: u64,
    /// 成功请求数
    pub successful_requests: u64,
    /// 失败请求数
    pub failed_requests: u64,
}

/// IPC 服务器
///
/// 提供进程间通信服务，支持远程客户端通过 Socket 与内核交互
pub struct IpcServer {
    /// 配置
    config: IpcConfig,
    /// ChipsCore 引用
    core: Arc<crate::api::sdk::ChipsCore>,
    /// 运行状态
    running: Arc<AtomicBool>,
    /// 统计信息
    stats: Arc<RwLock<IpcServerStats>>,
    /// 活跃连接计数
    active_connections: Arc<AtomicU64>,
    /// 关闭通知
    shutdown_tx: broadcast::Sender<()>,
}

impl IpcServer {
    /// 创建新的 IPC 服务器
    ///
    /// # Arguments
    ///
    /// * `config` - IPC 配置
    /// * `core` - ChipsCore 实例引用
    pub fn new(config: IpcConfig, core: Arc<crate::api::sdk::ChipsCore>) -> Self {
        let (shutdown_tx, _) = broadcast::channel(1);
        Self {
            config,
            core,
            running: Arc::new(AtomicBool::new(false)),
            stats: Arc::new(RwLock::new(IpcServerStats::default())),
            active_connections: Arc::new(AtomicU64::new(0)),
            shutdown_tx,
        }
    }

    /// 启动 IPC 服务
    ///
    /// 开始监听连接并处理请求
    pub async fn start(&self) -> Result<()> {
        if self.running.swap(true, Ordering::SeqCst) {
            return Err(CoreError::InitFailed("IPC 服务器已在运行".to_string()));
        }

        match &self.config.transport {
            #[cfg(unix)]
            IpcTransport::UnixSocket(path) => {
                self.start_unix_server(path.clone()).await
            }
            IpcTransport::Tcp(host, port) => {
                self.start_tcp_server(host.clone(), *port).await
            }
        }
    }

    /// 启动 Unix Socket 服务器
    #[cfg(unix)]
    async fn start_unix_server(&self, path: PathBuf) -> Result<()> {
        use tokio::net::UnixListener;

        // 删除已存在的 socket 文件
        if path.exists() {
            std::fs::remove_file(&path).map_err(|e| {
                CoreError::InitFailed(format!("无法删除旧的 socket 文件: {}", e))
            })?;
        }

        let listener = UnixListener::bind(&path).map_err(|e| {
            CoreError::InitFailed(format!("无法绑定 Unix Socket '{}': {}", path.display(), e))
        })?;

        info!("IPC 服务器已启动 (Unix Socket: {})", path.display());

        let running = self.running.clone();
        let config = self.config.clone();
        let core = self.core.clone();
        let stats = self.stats.clone();
        let active_connections = self.active_connections.clone();
        let mut shutdown_rx = self.shutdown_tx.subscribe();

        tokio::spawn(async move {
            loop {
                tokio::select! {
                    result = listener.accept() => {
                        match result {
                            Ok((stream, _)) => {
                                // 检查连接数限制
                                let current = active_connections.load(Ordering::SeqCst);
                                if current >= config.max_connections as u64 {
                                    warn!("达到最大连接数限制，拒绝新连接");
                                    continue;
                                }

                                active_connections.fetch_add(1, Ordering::SeqCst);
                                {
                                    let mut s = stats.write().await;
                                    s.total_connections += 1;
                                    s.active_connections = active_connections.load(Ordering::SeqCst);
                                }

                                let core = core.clone();
                                let config = config.clone();
                                let stats = stats.clone();
                                let active_connections = active_connections.clone();

                                tokio::spawn(async move {
                                    Self::handle_unix_connection(stream, core, config, stats.clone()).await;
                                    active_connections.fetch_sub(1, Ordering::SeqCst);
                                    {
                                        let mut s = stats.write().await;
                                        s.active_connections = active_connections.load(Ordering::SeqCst);
                                    }
                                });
                            }
                            Err(e) => {
                                error!("接受连接失败: {}", e);
                            }
                        }
                    }
                    _ = shutdown_rx.recv() => {
                        info!("IPC 服务器正在关闭...");
                        break;
                    }
                }

                if !running.load(Ordering::SeqCst) {
                    break;
                }
            }

            // 清理 socket 文件
            let _ = std::fs::remove_file(&path);
            info!("IPC 服务器已停止");
        });

        Ok(())
    }

    /// 处理 Unix Socket 连接
    #[cfg(unix)]
    async fn handle_unix_connection(
        stream: tokio::net::UnixStream,
        core: Arc<crate::api::sdk::ChipsCore>,
        config: IpcConfig,
        stats: Arc<RwLock<IpcServerStats>>,
    ) {
        let (reader, writer) = stream.into_split();
        let reader = BufReader::new(reader);
        let writer = Arc::new(tokio::sync::Mutex::new(writer));

        Self::handle_connection_inner(reader, writer, core, config, stats).await;
    }

    /// 启动 TCP 服务器
    async fn start_tcp_server(&self, host: String, port: u16) -> Result<()> {
        use tokio::net::TcpListener;

        let addr = format!("{}:{}", host, port);
        let listener = TcpListener::bind(&addr).await.map_err(|e| {
            CoreError::InitFailed(format!("无法绑定 TCP 地址 '{}': {}", addr, e))
        })?;

        info!("IPC 服务器已启动 (TCP: {})", addr);

        let running = self.running.clone();
        let config = self.config.clone();
        let core = self.core.clone();
        let stats = self.stats.clone();
        let active_connections = self.active_connections.clone();
        let mut shutdown_rx = self.shutdown_tx.subscribe();

        tokio::spawn(async move {
            loop {
                tokio::select! {
                    result = listener.accept() => {
                        match result {
                            Ok((stream, addr)) => {
                                debug!("新的 TCP 连接: {}", addr);

                                // 检查连接数限制
                                let current = active_connections.load(Ordering::SeqCst);
                                if current >= config.max_connections as u64 {
                                    warn!("达到最大连接数限制，拒绝新连接");
                                    continue;
                                }

                                active_connections.fetch_add(1, Ordering::SeqCst);
                                {
                                    let mut s = stats.write().await;
                                    s.total_connections += 1;
                                    s.active_connections = active_connections.load(Ordering::SeqCst);
                                }

                                let core = core.clone();
                                let config = config.clone();
                                let stats = stats.clone();
                                let active_connections = active_connections.clone();

                                tokio::spawn(async move {
                                    Self::handle_tcp_connection(stream, core, config, stats.clone()).await;
                                    active_connections.fetch_sub(1, Ordering::SeqCst);
                                    {
                                        let mut s = stats.write().await;
                                        s.active_connections = active_connections.load(Ordering::SeqCst);
                                    }
                                });
                            }
                            Err(e) => {
                                error!("接受 TCP 连接失败: {}", e);
                            }
                        }
                    }
                    _ = shutdown_rx.recv() => {
                        info!("IPC 服务器正在关闭...");
                        break;
                    }
                }

                if !running.load(Ordering::SeqCst) {
                    break;
                }
            }

            info!("IPC TCP 服务器已停止");
        });

        Ok(())
    }

    /// 处理 TCP 连接
    async fn handle_tcp_connection(
        stream: tokio::net::TcpStream,
        core: Arc<crate::api::sdk::ChipsCore>,
        config: IpcConfig,
        stats: Arc<RwLock<IpcServerStats>>,
    ) {
        let (reader, writer) = stream.into_split();
        let reader = BufReader::new(reader);
        let writer = Arc::new(tokio::sync::Mutex::new(writer));

        Self::handle_connection_inner(reader, writer, core, config, stats).await;
    }

    /// 处理连接的内部实现
    async fn handle_connection_inner<R, W>(
        mut reader: BufReader<R>,
        writer: Arc<tokio::sync::Mutex<W>>,
        core: Arc<crate::api::sdk::ChipsCore>,
        config: IpcConfig,
        stats: Arc<RwLock<IpcServerStats>>,
    ) where
        R: tokio::io::AsyncRead + Unpin,
        W: tokio::io::AsyncWrite + Unpin,
    {
        let mut line = String::new();

        loop {
            line.clear();

            // 读取请求
            let read_result = tokio::time::timeout(
                Duration::from_millis(config.request_timeout_ms),
                reader.read_line(&mut line),
            )
            .await;

            match read_result {
                Ok(Ok(0)) => {
                    // 连接关闭
                    debug!("客户端断开连接");
                    break;
                }
                Ok(Ok(_)) => {
                    // 解析请求
                    match IpcRequest::from_line(&line) {
                        Ok(request) => {
                            // 处理请求
                            let response = Self::handle_request(&request, &core, &stats).await;

                            // 发送响应
                            if let Ok(response_line) = response.to_line() {
                                let mut w = writer.lock().await;
                                if let Err(e) = w.write_all(response_line.as_bytes()).await {
                                    error!("发送响应失败: {}", e);
                                    break;
                                }
                                if let Err(e) = w.flush().await {
                                    error!("刷新缓冲区失败: {}", e);
                                    break;
                                }
                            }

                            // 检查是否是断开连接请求
                            if request.message_type == IpcMessageType::Disconnect {
                                debug!("客户端请求断开连接");
                                break;
                            }
                        }
                        Err(e) => {
                            warn!("解析请求失败: {}", e);
                            let response = IpcResponse::error("unknown", format!("解析请求失败: {}", e));
                            if let Ok(response_line) = response.to_line() {
                                let mut w = writer.lock().await;
                                let _ = w.write_all(response_line.as_bytes()).await;
                                let _ = w.flush().await;
                            }
                        }
                    }
                }
                Ok(Err(e)) => {
                    error!("读取请求失败: {}", e);
                    break;
                }
                Err(_) => {
                    debug!("读取请求超时");
                    // 超时不断开，继续等待
                }
            }
        }
    }

    /// 处理单个请求
    async fn handle_request(
        request: &IpcRequest,
        core: &Arc<crate::api::sdk::ChipsCore>,
        stats: &Arc<RwLock<IpcServerStats>>,
    ) -> IpcResponse {
        {
            let mut s = stats.write().await;
            s.total_requests += 1;
        }

        let result = match request.message_type {
            IpcMessageType::Route => Self::handle_route(request, core).await,
            IpcMessageType::Publish => Self::handle_publish(request, core).await,
            IpcMessageType::Subscribe => Self::handle_subscribe(request, core).await,
            IpcMessageType::Unsubscribe => Self::handle_unsubscribe(request, core).await,
            IpcMessageType::Heartbeat => Self::handle_heartbeat(request),
            IpcMessageType::ConfigGet => Self::handle_config_get(request, core).await,
            IpcMessageType::ConfigSet => Self::handle_config_set(request, core).await,
            IpcMessageType::Status => Self::handle_status(request, core).await,
            IpcMessageType::HeartbeatAck => IpcResponse::success_empty(&request.id),
            IpcMessageType::Disconnect => IpcResponse::success_empty(&request.id),
        };

        {
            let mut s = stats.write().await;
            if result.success {
                s.successful_requests += 1;
            } else {
                s.failed_requests += 1;
            }
        }

        result
    }

    /// 处理路由请求
    async fn handle_route(
        request: &IpcRequest,
        core: &Arc<crate::api::sdk::ChipsCore>,
    ) -> IpcResponse {
        let sender = request.payload.get("sender")
            .and_then(|v| v.as_str())
            .unwrap_or("ipc_client");
        let action = request.payload.get("action")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let params = request.payload.get("params")
            .cloned()
            .unwrap_or(Value::Null);

        if action.is_empty() {
            return IpcResponse::error(&request.id, "action 不能为空");
        }

        let route_request = RouteRequest::new(sender, action, params);
        let route_response = core.route(route_request).await;

        if route_response.is_success() {
            IpcResponse::success(&request.id, serde_json::to_value(&route_response).unwrap_or(Value::Null))
        } else {
            let error_msg = route_response.error
                .map(|e| e.message)
                .unwrap_or_else(|| "路由失败".to_string());
            IpcResponse::error(&request.id, error_msg)
        }
    }

    /// 处理发布事件请求
    async fn handle_publish(
        request: &IpcRequest,
        core: &Arc<crate::api::sdk::ChipsCore>,
    ) -> IpcResponse {
        let event_type = request.payload.get("event_type")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let sender = request.payload.get("sender")
            .and_then(|v| v.as_str())
            .unwrap_or("ipc_client");
        let data = request.payload.get("data")
            .cloned()
            .unwrap_or(Value::Null);

        if event_type.is_empty() {
            return IpcResponse::error(&request.id, "event_type 不能为空");
        }

        let event = Event::new(event_type, sender, data);
        match core.publish(event).await {
            Ok(count) => IpcResponse::success(&request.id, serde_json::json!({ "subscribers": count })),
            Err(e) => IpcResponse::error(&request.id, e.to_string()),
        }
    }

    /// 处理订阅请求
    async fn handle_subscribe(
        request: &IpcRequest,
        core: &Arc<crate::api::sdk::ChipsCore>,
    ) -> IpcResponse {
        let subscriber_id = request.payload.get("subscriber_id")
            .and_then(|v| v.as_str())
            .unwrap_or("ipc_subscriber");
        let event_type = request.payload.get("event_type")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        if event_type.is_empty() {
            return IpcResponse::error(&request.id, "event_type 不能为空");
        }

        // 注意：IPC 订阅需要特殊处理，因为无法直接传递回调
        // 这里只返回订阅成功，实际事件推送需要通过其他机制
        match core.subscribe(subscriber_id, event_type, Arc::new(|_| {})).await {
            Ok(subscription_id) => {
                IpcResponse::success(&request.id, serde_json::json!({ "subscription_id": subscription_id }))
            }
            Err(e) => IpcResponse::error(&request.id, e.to_string()),
        }
    }

    /// 处理取消订阅请求
    async fn handle_unsubscribe(
        request: &IpcRequest,
        core: &Arc<crate::api::sdk::ChipsCore>,
    ) -> IpcResponse {
        let subscription_id = request.payload.get("subscription_id")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        if subscription_id.is_empty() {
            return IpcResponse::error(&request.id, "subscription_id 不能为空");
        }

        match core.unsubscribe(subscription_id).await {
            Ok(()) => IpcResponse::success_empty(&request.id),
            Err(e) => IpcResponse::error(&request.id, e.to_string()),
        }
    }

    /// 处理心跳请求
    fn handle_heartbeat(request: &IpcRequest) -> IpcResponse {
        IpcResponse::success(&request.id, serde_json::json!({
            "type": "heartbeat_ack",
            "server_time": Utc::now().to_rfc3339()
        }))
    }

    /// 处理配置读取请求
    async fn handle_config_get(
        request: &IpcRequest,
        core: &Arc<crate::api::sdk::ChipsCore>,
    ) -> IpcResponse {
        let key = request.payload.get("key")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        if key.is_empty() {
            return IpcResponse::error(&request.id, "key 不能为空");
        }

        let value: Option<Value> = core.get_config(key).await;
        match value {
            Some(v) => IpcResponse::success(&request.id, v),
            None => IpcResponse::error(&request.id, format!("配置项 '{}' 不存在", key)),
        }
    }

    /// 处理配置设置请求
    async fn handle_config_set(
        request: &IpcRequest,
        core: &Arc<crate::api::sdk::ChipsCore>,
    ) -> IpcResponse {
        let key = request.payload.get("key")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let value = request.payload.get("value");

        if key.is_empty() {
            return IpcResponse::error(&request.id, "key 不能为空");
        }

        match value {
            Some(v) => {
                match core.set_config(key, v.clone()).await {
                    Ok(()) => IpcResponse::success_empty(&request.id),
                    Err(e) => IpcResponse::error(&request.id, e.to_string()),
                }
            }
            None => IpcResponse::error(&request.id, "value 不能为空"),
        }
    }

    /// 处理状态请求
    async fn handle_status(
        request: &IpcRequest,
        core: &Arc<crate::api::sdk::ChipsCore>,
    ) -> IpcResponse {
        let health = core.health().await;
        let stats = core.router_stats();

        IpcResponse::success(&request.id, serde_json::json!({
            "state": format!("{:?}", health.state),
            "uptime_secs": health.uptime_secs,
            "router_running": health.router_running,
            "total_requests": stats.total_requests,
            "success_rate": stats.success_rate,
            "event_subscriptions": health.event_subscriptions,
            "events_dispatched": health.events_dispatched,
        }))
    }

    /// 停止 IPC 服务
    pub async fn stop(&self) -> Result<()> {
        if !self.running.swap(false, Ordering::SeqCst) {
            return Ok(());
        }

        // 发送关闭信号
        let _ = self.shutdown_tx.send(());

        info!("IPC 服务器已停止");
        Ok(())
    }

    /// 检查是否正在运行
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    /// 获取统计信息
    pub async fn stats(&self) -> IpcServerStats {
        self.stats.read().await.clone()
    }

    /// 获取当前活跃连接数
    pub fn active_connections(&self) -> u64 {
        self.active_connections.load(Ordering::SeqCst)
    }
}

// ============================================================================
// IPC 客户端
// ============================================================================

/// IPC 客户端
///
/// 用于连接到 IPC 服务器并发送请求
pub struct IpcClient {
    /// 配置
    config: IpcConfig,
    /// 内部连接状态
    state: IpcClientState,
}

/// 客户端连接状态
enum IpcClientState {
    /// 未连接
    Disconnected,
    /// Unix Socket 连接
    #[cfg(unix)]
    UnixConnected {
        reader: BufReader<tokio::net::unix::OwnedReadHalf>,
        writer: tokio::net::unix::OwnedWriteHalf,
    },
    /// TCP 连接
    TcpConnected {
        reader: BufReader<tokio::net::tcp::OwnedReadHalf>,
        writer: tokio::net::tcp::OwnedWriteHalf,
    },
}

impl IpcClient {
    /// 连接到 IPC 服务器
    ///
    /// # Arguments
    ///
    /// * `config` - IPC 配置
    ///
    /// # Returns
    ///
    /// 返回连接成功的客户端实例
    pub async fn connect(config: IpcConfig) -> Result<Self> {
        let state = match &config.transport {
            #[cfg(unix)]
            IpcTransport::UnixSocket(path) => {
                use tokio::net::UnixStream;

                let stream = tokio::time::timeout(
                    Duration::from_millis(config.connection_timeout_ms),
                    UnixStream::connect(path),
                )
                .await
                .map_err(|_| CoreError::Timeout("连接超时".to_string()))?
                .map_err(|e| CoreError::InitFailed(format!("连接失败: {}", e)))?;

                let (reader, writer) = stream.into_split();
                IpcClientState::UnixConnected {
                    reader: BufReader::new(reader),
                    writer,
                }
            }
            IpcTransport::Tcp(host, port) => {
                use tokio::net::TcpStream;

                let addr = format!("{}:{}", host, port);
                let stream = tokio::time::timeout(
                    Duration::from_millis(config.connection_timeout_ms),
                    TcpStream::connect(&addr),
                )
                .await
                .map_err(|_| CoreError::Timeout("连接超时".to_string()))?
                .map_err(|e| CoreError::InitFailed(format!("连接失败: {}", e)))?;

                let (reader, writer) = stream.into_split();
                IpcClientState::TcpConnected {
                    reader: BufReader::new(reader),
                    writer,
                }
            }
        };

        Ok(Self { config, state })
    }

    /// 发送请求并等待响应
    ///
    /// # Arguments
    ///
    /// * `request` - IPC 请求
    ///
    /// # Returns
    ///
    /// 返回服务器响应
    pub async fn send_request(&mut self, request: IpcRequest) -> Result<IpcResponse> {
        let request_line = request.to_line()?;

        match &mut self.state {
            IpcClientState::Disconnected => {
                Err(CoreError::InitFailed("未连接到服务器".to_string()))
            }
            #[cfg(unix)]
            IpcClientState::UnixConnected { reader, writer } => {
                // 发送请求
                writer.write_all(request_line.as_bytes()).await?;
                writer.flush().await?;

                // 读取响应
                let mut response_line = String::new();
                tokio::time::timeout(
                    Duration::from_millis(self.config.request_timeout_ms),
                    reader.read_line(&mut response_line),
                )
                .await
                .map_err(|_| CoreError::Timeout("请求超时".to_string()))??;

                IpcResponse::from_line(&response_line)
            }
            IpcClientState::TcpConnected { reader, writer } => {
                // 发送请求
                writer.write_all(request_line.as_bytes()).await?;
                writer.flush().await?;

                // 读取响应
                let mut response_line = String::new();
                tokio::time::timeout(
                    Duration::from_millis(self.config.request_timeout_ms),
                    reader.read_line(&mut response_line),
                )
                .await
                .map_err(|_| CoreError::Timeout("请求超时".to_string()))??;

                IpcResponse::from_line(&response_line)
            }
        }
    }

    /// 发送路由请求
    pub async fn route(
        &mut self,
        sender: impl Into<String>,
        action: impl Into<String>,
        params: Value,
    ) -> Result<IpcResponse> {
        let request = IpcRequest::route(sender, action, params);
        self.send_request(request).await
    }

    /// 发布事件
    pub async fn publish(
        &mut self,
        event_type: impl Into<String>,
        sender: impl Into<String>,
        data: Value,
    ) -> Result<IpcResponse> {
        let request = IpcRequest::publish(event_type, sender, data);
        self.send_request(request).await
    }

    /// 获取配置
    pub async fn get_config(&mut self, key: impl Into<String>) -> Result<IpcResponse> {
        let request = IpcRequest::config_get(key);
        self.send_request(request).await
    }

    /// 设置配置
    pub async fn set_config(&mut self, key: impl Into<String>, value: Value) -> Result<IpcResponse> {
        let request = IpcRequest::config_set(key, value);
        self.send_request(request).await
    }

    /// 获取服务器状态
    pub async fn status(&mut self) -> Result<IpcResponse> {
        let request = IpcRequest::status();
        self.send_request(request).await
    }

    /// 发送心跳
    pub async fn heartbeat(&mut self) -> Result<IpcResponse> {
        let request = IpcRequest::heartbeat();
        self.send_request(request).await
    }

    /// 断开连接
    pub async fn disconnect(&mut self) -> Result<()> {
        if matches!(self.state, IpcClientState::Disconnected) {
            return Ok(());
        }

        // 发送断开连接请求
        let _ = self.send_request(IpcRequest::disconnect()).await;

        self.state = IpcClientState::Disconnected;
        Ok(())
    }

    /// 检查是否已连接
    pub fn is_connected(&self) -> bool {
        !matches!(self.state, IpcClientState::Disconnected)
    }
}

// ============================================================================
// Builder 模式
// ============================================================================

/// IPC 配置构建器
#[derive(Debug, Default)]
pub struct IpcConfigBuilder {
    config: IpcConfig,
}

impl IpcConfigBuilder {
    /// 创建新的构建器
    pub fn new() -> Self {
        Self::default()
    }

    /// 设置 Unix Socket 路径
    #[cfg(unix)]
    pub fn unix_socket(mut self, path: impl Into<PathBuf>) -> Self {
        self.config.transport = IpcTransport::UnixSocket(path.into());
        self
    }

    /// 设置 TCP 地址
    pub fn tcp(mut self, host: impl Into<String>, port: u16) -> Self {
        self.config.transport = IpcTransport::Tcp(host.into(), port);
        self
    }

    /// 设置最大连接数
    pub fn max_connections(mut self, max: usize) -> Self {
        self.config.max_connections = max;
        self
    }

    /// 设置连接超时
    pub fn connection_timeout_ms(mut self, timeout_ms: u64) -> Self {
        self.config.connection_timeout_ms = timeout_ms;
        self
    }

    /// 设置请求超时
    pub fn request_timeout_ms(mut self, timeout_ms: u64) -> Self {
        self.config.request_timeout_ms = timeout_ms;
        self
    }

    /// 启用心跳
    pub fn enable_heartbeat(mut self, interval_secs: u64) -> Self {
        self.config.enable_heartbeat = true;
        self.config.heartbeat_interval_secs = interval_secs;
        self
    }

    /// 禁用心跳
    pub fn disable_heartbeat(mut self) -> Self {
        self.config.enable_heartbeat = false;
        self
    }

    /// 设置缓冲区大小
    pub fn buffer_size(mut self, size: usize) -> Self {
        self.config.buffer_size = size;
        self
    }

    /// 构建配置
    pub fn build(self) -> IpcConfig {
        self.config
    }
}

// ============================================================================
// 单元测试
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // ------------------------------------------------------------------------
    // IpcConfig 测试
    // ------------------------------------------------------------------------

    #[test]
    fn test_ipc_config_default() {
        let config = IpcConfig::default();
        assert_eq!(config.max_connections, 100);
        assert_eq!(config.connection_timeout_ms, 5000);
        assert_eq!(config.request_timeout_ms, 30000);
        assert!(config.enable_heartbeat);
    }

    #[test]
    fn test_ipc_config_tcp() {
        let config = IpcConfig::tcp("127.0.0.1", 8080);
        assert!(matches!(config.transport, IpcTransport::Tcp(_, 8080)));
    }

    #[cfg(unix)]
    #[test]
    fn test_ipc_config_unix_socket() {
        let config = IpcConfig::unix_socket("/tmp/test.sock");
        assert!(matches!(config.transport, IpcTransport::UnixSocket(_)));
    }

    #[test]
    fn test_ipc_config_builder() {
        let config = IpcConfigBuilder::new()
            .tcp("localhost", 9527)
            .max_connections(50)
            .connection_timeout_ms(10000)
            .disable_heartbeat()
            .build();

        assert_eq!(config.max_connections, 50);
        assert_eq!(config.connection_timeout_ms, 10000);
        assert!(!config.enable_heartbeat);
    }

    // ------------------------------------------------------------------------
    // IpcRequest 测试
    // ------------------------------------------------------------------------

    #[test]
    fn test_ipc_request_route() {
        let request = IpcRequest::route("client", "test.action", json!({"key": "value"}));
        
        assert_eq!(request.message_type, IpcMessageType::Route);
        assert_eq!(request.payload["sender"], "client");
        assert_eq!(request.payload["action"], "test.action");
    }

    #[test]
    fn test_ipc_request_publish() {
        let request = IpcRequest::publish("test.event", "sender", json!({"data": 123}));
        
        assert_eq!(request.message_type, IpcMessageType::Publish);
        assert_eq!(request.payload["event_type"], "test.event");
    }

    #[test]
    fn test_ipc_request_subscribe() {
        let request = IpcRequest::subscribe("subscriber", "event.*");
        
        assert_eq!(request.message_type, IpcMessageType::Subscribe);
        assert_eq!(request.payload["subscriber_id"], "subscriber");
        assert_eq!(request.payload["event_type"], "event.*");
    }

    #[test]
    fn test_ipc_request_heartbeat() {
        let request = IpcRequest::heartbeat();
        assert_eq!(request.message_type, IpcMessageType::Heartbeat);
    }

    #[test]
    fn test_ipc_request_config() {
        let get_request = IpcRequest::config_get("router.worker_count");
        assert_eq!(get_request.message_type, IpcMessageType::ConfigGet);
        assert_eq!(get_request.payload["key"], "router.worker_count");

        let set_request = IpcRequest::config_set("router.worker_count", json!(8));
        assert_eq!(set_request.message_type, IpcMessageType::ConfigSet);
        assert_eq!(set_request.payload["value"], 8);
    }

    #[test]
    fn test_ipc_request_serialization() {
        let request = IpcRequest::route("client", "test.action", json!({}));
        let line = request.to_line().unwrap();
        
        assert!(line.ends_with('\n'));
        
        let parsed = IpcRequest::from_line(&line).unwrap();
        assert_eq!(parsed.id, request.id);
        assert_eq!(parsed.message_type, request.message_type);
    }

    // ------------------------------------------------------------------------
    // IpcResponse 测试
    // ------------------------------------------------------------------------

    #[test]
    fn test_ipc_response_success() {
        let response = IpcResponse::success("req-123", json!({"result": "ok"}));
        
        assert!(response.success);
        assert_eq!(response.request_id, "req-123");
        assert!(response.data.is_some());
        assert!(response.error.is_none());
    }

    #[test]
    fn test_ipc_response_error() {
        let response = IpcResponse::error("req-123", "Something went wrong");
        
        assert!(!response.success);
        assert_eq!(response.request_id, "req-123");
        assert!(response.data.is_none());
        assert!(response.error.is_some());
    }

    #[test]
    fn test_ipc_response_serialization() {
        let response = IpcResponse::success("req-123", json!({"result": "ok"}));
        let line = response.to_line().unwrap();
        
        assert!(line.ends_with('\n'));
        
        let parsed = IpcResponse::from_line(&line).unwrap();
        assert_eq!(parsed.request_id, response.request_id);
        assert_eq!(parsed.success, response.success);
    }

    // ------------------------------------------------------------------------
    // IpcEvent 测试
    // ------------------------------------------------------------------------

    #[test]
    fn test_ipc_event_from_event() {
        let event = Event::new("test.event", "sender", json!({"key": "value"}));
        let ipc_event: IpcEvent = event.clone().into();
        
        assert_eq!(ipc_event.event_id, event.event_id);
        assert_eq!(ipc_event.event_type, event.event_type);
        assert_eq!(ipc_event.sender, event.sender);
    }

    // ------------------------------------------------------------------------
    // IpcMessageType 测试
    // ------------------------------------------------------------------------

    #[test]
    fn test_message_type_serialization() {
        let types = vec![
            IpcMessageType::Route,
            IpcMessageType::Publish,
            IpcMessageType::Subscribe,
            IpcMessageType::Unsubscribe,
            IpcMessageType::Heartbeat,
            IpcMessageType::ConfigGet,
            IpcMessageType::ConfigSet,
            IpcMessageType::Status,
            IpcMessageType::Disconnect,
        ];

        for msg_type in types {
            let json = serde_json::to_string(&msg_type).unwrap();
            let parsed: IpcMessageType = serde_json::from_str(&json).unwrap();
            assert_eq!(parsed, msg_type);
        }
    }

    // ------------------------------------------------------------------------
    // IpcServerStats 测试
    // ------------------------------------------------------------------------

    #[test]
    fn test_ipc_server_stats_default() {
        let stats = IpcServerStats::default();
        assert_eq!(stats.total_connections, 0);
        assert_eq!(stats.active_connections, 0);
        assert_eq!(stats.total_requests, 0);
        assert_eq!(stats.successful_requests, 0);
        assert_eq!(stats.failed_requests, 0);
    }
}
