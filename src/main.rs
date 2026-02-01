//! Chips Core 命令行入口
//!
//! 薯片微内核的命令行工具，提供启动、管理和调试功能。
//!
//! # 命令概览
//!
//! - `start` - 启动内核
//! - `version` - 显示版本信息
//! - `check-config` - 验证配置文件
//! - `stats` - 显示统计信息
//! - `list-modules` - 列出已注册的模块
//! - `routes` - 查看路由表
//! - `route` - 发送路由请求
//!
//! # 使用示例
//!
//! ```bash
//! # 启动内核
//! chips-core start
//!
//! # 使用自定义配置文件启动
//! chips-core -c my-config.yaml start
//!
//! # 开发模式启动
//! chips-core --dev start
//!
//! # 检查配置文件
//! chips-core check-config -c config.yaml
//!
//! # 查看版本
//! chips-core version
//! ```

use clap::{Parser, Subcommand};
use std::path::PathBuf;
use tokio::signal;
use tracing::{info, Level};
use tracing_subscriber::{fmt, EnvFilter};

use chips_core::{ChipsCore, CoreConfig, RouteRequest};

/// Chips Core - 薯片微内核
///
/// 薯片生态的核心组件，提供中心路由、模块管理、事件总线和配置管理功能。
#[derive(Parser)]
#[command(name = "chips-core")]
#[command(version, about = "薯片生态的核心微内核", long_about = None)]
#[command(author = "Chips Team")]
#[command(propagate_version = true)]
struct Cli {
    /// 配置文件路径
    #[arg(short, long, default_value = "config.yaml", global = true)]
    config: PathBuf,

    /// 日志级别 (trace, debug, info, warn, error)
    #[arg(short, long, default_value = "info", global = true)]
    log_level: String,

    /// 开发模式（启用更详细的日志和调试功能）
    #[arg(long, global = true)]
    dev: bool,

    /// 子命令
    #[command(subcommand)]
    command: Option<Commands>,
}

/// 可用的子命令
#[derive(Subcommand)]
enum Commands {
    /// 启动内核
    ///
    /// 启动薯片微内核，加载配置并运行所有子系统。
    /// 按 Ctrl+C 可优雅关闭内核。
    Start,

    /// 查看版本信息
    ///
    /// 显示详细的版本信息，包括库版本、协议版本等。
    Version,

    /// 验证配置文件
    ///
    /// 检查配置文件是否有效，并显示解析后的配置内容。
    CheckConfig {
        /// 配置文件路径（不指定则使用全局 -c 选项）
        #[arg(short, long)]
        config: Option<PathBuf>,
    },

    /// 查看统计信息
    ///
    /// 连接到运行中的内核，获取并显示运行统计信息。
    Stats,

    /// 列出已注册的模块
    ///
    /// 显示所有已注册模块的信息，包括模块 ID、状态等。
    ListModules,

    /// 查看路由表
    ///
    /// 显示当前路由表中所有已注册的路由条目。
    Routes,

    /// 发送路由请求
    ///
    /// 向内核发送一个路由请求，用于测试和调试。
    Route {
        /// 目标动作（格式: module.action）
        #[arg(short, long)]
        action: String,

        /// 请求参数（JSON 格式）
        #[arg(short, long, default_value = "{}")]
        params: String,

        /// 发送方标识
        #[arg(short, long, default_value = "cli")]
        sender: String,
    },
}

/// 初始化日志系统
///
/// 根据日志级别和开发模式配置 tracing 日志。
fn init_logging(level: &str, dev_mode: bool) {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| {
            let level = match level.to_lowercase().as_str() {
                "trace" => Level::TRACE,
                "debug" => Level::DEBUG,
                "info" => Level::INFO,
                "warn" => Level::WARN,
                "error" => Level::ERROR,
                _ => Level::INFO,
            };
            EnvFilter::new(format!("chips_core={}", level))
        });

    let builder = fmt().with_env_filter(filter).with_target(true);

    if dev_mode {
        // 开发模式：显示更多信息
        builder
            .with_thread_ids(true)
            .with_file(true)
            .with_line_number(true)
            .init();
    } else {
        // 生产模式：简洁输出
        builder
            .with_thread_ids(false)
            .with_file(false)
            .with_line_number(false)
            .init();
    }
}

/// 启动内核
async fn run_start(config: CoreConfig) -> Result<(), Box<dyn std::error::Error>> {
    info!("启动薯片微内核...");

    let mut core = ChipsCore::new(config).await?;
    core.start().await?;

    println!();
    println!("╔════════════════════════════════════════════════════════╗");
    println!("║          薯片微内核已启动 (Chips Core Started)         ║");
    println!("╠════════════════════════════════════════════════════════╣");
    println!("║  版本: {}                                           ║", chips_core::VERSION);
    println!("║  协议: {}                                           ║", chips_core::PROTOCOL_VERSION);
    println!("║                                                        ║");
    println!("║  按 Ctrl+C 优雅关闭内核                                ║");
    println!("╚════════════════════════════════════════════════════════╝");
    println!();

    // 等待关闭信号
    signal::ctrl_c().await?;

    println!();
    info!("收到关闭信号，正在优雅关闭...");
    core.shutdown().await?;
    info!("薯片微内核已关闭");

    Ok(())
}

/// 检查配置文件
async fn check_config(path: &PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    println!("检查配置文件: {}", path.display());
    println!();

    if !path.exists() {
        println!("⚠️  警告: 配置文件不存在，将使用默认配置");
        println!();
        print_default_config();
        return Ok(());
    }

    match CoreConfig::from_file(path).await {
        Ok(config) => {
            println!("✅ 配置文件有效！");
            println!();
            println!("配置内容:");
            println!("────────────────────────────────────────");
            println!("  [路由器配置]");
            println!("    工作线程数:     {}", config.router.worker_count);
            println!("    最大并发请求数: {}", config.router.max_concurrent);
            println!("    队列大小:       {}", config.router.queue_size);
            println!("    默认超时:       {} ms", config.router.default_timeout_ms);
            println!();
            println!("  [日志配置]");
            println!("    日志级别:       {}", config.logging.level);
            println!("    文件输出:       {}", if config.logging.file_output { "是" } else { "否" });
            println!("    JSON 格式:      {}", if config.logging.json_format { "是" } else { "否" });
            println!();
            println!("  [模块配置]");
            println!("    模块目录:       {:?}", config.modules.module_dirs);
            println!("    热插拔:         {}", if config.modules.hot_reload { "启用" } else { "禁用" });
            println!("    健康检查间隔:   {} 秒", config.modules.health_check_interval_secs);
            println!();
            println!("  [其他]");
            println!("    开发模式:       {}", if config.dev_mode { "是" } else { "否" });
            if let Some(ref data_dir) = config.data_dir {
                println!("    数据目录:       {}", data_dir.display());
            }
            println!("────────────────────────────────────────");
            Ok(())
        }
        Err(e) => {
            println!("❌ 配置文件无效: {}", e);
            Err(Box::new(e))
        }
    }
}

/// 打印默认配置
fn print_default_config() {
    let config = CoreConfig::default();
    println!("默认配置:");
    println!("────────────────────────────────────────");
    println!("  [路由器配置]");
    println!("    工作线程数:     {}", config.router.worker_count);
    println!("    最大并发请求数: {}", config.router.max_concurrent);
    println!("    队列大小:       {}", config.router.queue_size);
    println!("    默认超时:       {} ms", config.router.default_timeout_ms);
    println!();
    println!("  [日志配置]");
    println!("    日志级别:       {}", config.logging.level);
    println!("────────────────────────────────────────");
}

/// 打印版本信息
fn print_version() {
    println!();
    println!("Chips Core - 薯片微内核");
    println!("═══════════════════════════════════════");
    println!("  版本:             {}", chips_core::VERSION);
    println!("  协议版本:         {}", chips_core::PROTOCOL_VERSION);
    println!("  最低兼容协议版本: {}", chips_core::MIN_PROTOCOL_VERSION);
    println!();
    println!("构建信息:");
    println!("  目标平台:         {}", std::env::consts::ARCH);
    println!("  操作系统:         {}", std::env::consts::OS);
    println!("═══════════════════════════════════════");
    println!();
}

/// 显示统计信息
async fn show_stats() {
    println!();
    println!("统计信息");
    println!("═══════════════════════════════════════");
    println!();
    println!("⚠️  此功能需要连接到运行中的内核");
    println!("    当前版本暂未实现 IPC 通信");
    println!();
    println!("提示: 请先使用 `chips-core start` 启动内核");
    println!("═══════════════════════════════════════");
    println!();
}

/// 列出已注册的模块
async fn list_modules() {
    println!();
    println!("已注册模块");
    println!("═══════════════════════════════════════");
    println!();
    println!("⚠️  此功能需要连接到运行中的内核");
    println!("    当前版本暂未实现 IPC 通信");
    println!();
    println!("提示: 启动内核后，模块将自动从配置的模块目录加载");
    println!("═══════════════════════════════════════");
    println!();
}

/// 显示路由表
async fn show_routes() {
    println!();
    println!("路由表");
    println!("═══════════════════════════════════════");
    println!();
    println!("⚠️  此功能需要连接到运行中的内核");
    println!("    当前版本暂未实现 IPC 通信");
    println!();
    println!("提示: 路由在模块加载时自动注册");
    println!("═══════════════════════════════════════");
    println!();
}

/// 发送路由请求
async fn send_route(action: &str, params: &str, sender: &str) -> Result<(), Box<dyn std::error::Error>> {
    println!();
    println!("发送路由请求");
    println!("═══════════════════════════════════════");
    println!("  动作:   {}", action);
    println!("  发送方: {}", sender);
    println!("  参数:   {}", params);
    println!();

    // 解析参数
    let params_value: serde_json::Value = serde_json::from_str(params)
        .map_err(|e| format!("参数 JSON 解析失败: {}", e))?;

    // 创建一个临时的内核实例来处理请求
    // 注意：这是一个简化实现，实际应通过 IPC 与运行中的内核通信
    let config = CoreConfig::default();
    let mut core = ChipsCore::new(config).await?;
    core.start().await?;

    // 创建并发送请求
    let request = RouteRequest::new(sender, action, params_value);
    let response = core.route(request).await;

    println!("响应:");
    println!("────────────────────────────────────────");
    println!("  状态码:   {}", response.status);
    println!("  耗时:     {} ms", response.elapsed_ms);
    
    if let Some(ref data) = response.data {
        println!("  数据:     {}", serde_json::to_string_pretty(data)?);
    }
    
    if let Some(ref err) = response.error {
        println!("  错误码:   {}", err.code);
        println!("  错误信息: {}", err.message);
    }
    println!("────────────────────────────────────────");

    core.shutdown().await?;
    
    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    // 初始化日志（Version 和 CheckConfig 命令不需要日志）
    let needs_logging = !matches!(
        cli.command,
        Some(Commands::Version) | Some(Commands::CheckConfig { .. })
    );

    if needs_logging {
        init_logging(&cli.log_level, cli.dev);
    }

    match cli.command {
        // 默认命令或 Start 命令：启动内核
        Some(Commands::Start) | None => {
            let config = load_config(&cli.config, cli.dev).await?;
            run_start(config).await?;
        }

        // 显示版本信息
        Some(Commands::Version) => {
            print_version();
        }

        // 检查配置文件
        Some(Commands::CheckConfig { config }) => {
            let config_path = config.unwrap_or(cli.config);
            check_config(&config_path).await?;
        }

        // 显示统计信息
        Some(Commands::Stats) => {
            show_stats().await;
        }

        // 列出已注册的模块
        Some(Commands::ListModules) => {
            list_modules().await;
        }

        // 显示路由表
        Some(Commands::Routes) => {
            show_routes().await;
        }

        // 发送路由请求
        Some(Commands::Route { action, params, sender }) => {
            send_route(&action, &params, &sender).await?;
        }
    }

    Ok(())
}

/// 加载配置文件
async fn load_config(config_path: &PathBuf, dev_mode: bool) -> Result<CoreConfig, Box<dyn std::error::Error>> {
    let config = if config_path.exists() {
        let mut config = CoreConfig::from_file(config_path).await?;
        if dev_mode {
            config.dev_mode = true;
        }
        info!("已加载配置文件: {}", config_path.display());
        config
    } else {
        info!("配置文件不存在 ({})，使用默认配置", config_path.display());
        let mut config = CoreConfig::default();
        if dev_mode {
            config.dev_mode = true;
        }
        config
    };

    Ok(config)
}
