//! Chips Core 命令行入口
//!
//! 薯片微内核的命令行工具。

use clap::{Parser, Subcommand};
use std::path::PathBuf;
use tokio::signal;
use tracing::{info, error, Level};
use tracing_subscriber::{fmt, EnvFilter};

use chips_core::{ChipsCore, CoreConfig};

/// Chips Core - 薯片微内核
#[derive(Parser)]
#[command(name = "chips-core")]
#[command(version, about = "薯片生态的核心微内核", long_about = None)]
struct Cli {
    /// 配置文件路径
    #[arg(short, long, default_value = "config.yaml")]
    config: PathBuf,

    /// 日志级别 (trace, debug, info, warn, error)
    #[arg(short, long, default_value = "info")]
    log_level: String,

    /// 开发模式
    #[arg(long)]
    dev: bool,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// 启动内核
    Start,
    
    /// 查看版本信息
    Version,
    
    /// 验证配置文件
    CheckConfig {
        /// 配置文件路径
        #[arg(short, long)]
        config: Option<PathBuf>,
    },
    
    /// 查看统计信息
    Stats,
}

fn init_logging(level: &str) {
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

    fmt()
        .with_env_filter(filter)
        .with_target(true)
        .with_thread_ids(false)
        .with_file(false)
        .with_line_number(false)
        .init();
}

async fn run_start(config: CoreConfig) -> Result<(), Box<dyn std::error::Error>> {
    info!("启动薯片微内核...");
    
    let mut core = ChipsCore::new(config).await?;
    core.start().await?;
    
    info!("薯片微内核已启动，按 Ctrl+C 关闭");
    
    // 等待关闭信号
    signal::ctrl_c().await?;
    
    info!("收到关闭信号，正在优雅关闭...");
    core.shutdown().await?;
    
    Ok(())
}

async fn check_config(path: &PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    println!("检查配置文件: {:?}", path);
    
    if !path.exists() {
        println!("警告: 配置文件不存在，将使用默认配置");
        return Ok(());
    }
    
    match CoreConfig::from_file(path).await {
        Ok(config) => {
            println!("配置文件有效！");
            println!("  路由器工作线程数: {}", config.router.worker_count);
            println!("  最大并发请求数: {}", config.router.max_concurrent);
            println!("  日志级别: {}", config.logging.level);
            println!("  模块目录: {:?}", config.modules.module_dirs);
            Ok(())
        }
        Err(e) => {
            println!("配置文件无效: {}", e);
            Err(Box::new(e))
        }
    }
}

fn print_version() {
    println!("Chips Core - 薯片微内核");
    println!("  版本: {}", chips_core::VERSION);
    println!("  协议版本: {}", chips_core::PROTOCOL_VERSION);
    println!("  最低兼容协议版本: {}", chips_core::MIN_PROTOCOL_VERSION);
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    
    // 初始化日志
    init_logging(&cli.log_level);
    
    match cli.command {
        Some(Commands::Start) | None => {
            // 加载配置
            let config = if cli.config.exists() {
                let mut config = CoreConfig::from_file(&cli.config).await?;
                if cli.dev {
                    config.dev_mode = true;
                }
                config
            } else {
                info!("配置文件不存在，使用默认配置");
                let mut config = CoreConfig::default();
                if cli.dev {
                    config.dev_mode = true;
                }
                config
            };
            
            run_start(config).await?;
        }
        
        Some(Commands::Version) => {
            print_version();
        }
        
        Some(Commands::CheckConfig { config }) => {
            let config_path = config.unwrap_or(cli.config);
            check_config(&config_path).await?;
        }
        
        Some(Commands::Stats) => {
            println!("统计信息功能尚未实现");
            // TODO: 连接到运行中的内核获取统计信息
        }
    }
    
    Ok(())
}
