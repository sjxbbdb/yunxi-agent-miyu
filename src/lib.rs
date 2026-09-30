//! YunXi 的根包:入口层(cli / config_tui / oobe / pm / question_tui)与两个二进制。
//! 下面四层住在 `crates/`(09-16 拆 crate):yunxi-base → yunxi-core → yunxi-engine → yunxi-hosts。
// 09-16 接口治理收口:死代码在生产构建里是警告,不再全局豁免;测试构建里 cfg(test) 的辅助函数照旧豁免。
#![cfg_attr(test, allow(dead_code))]

mod cli;
mod config_tui;
/// 功能表的数据源（引导与设置界面共用）。
mod feature_sources;
mod oobe;
mod pm;
mod question_tui;

use anyhow::Result;

/// 本次构建的唯一 id,根包 `build.rs` 算出(`YUNXI_BUILD_ID` 环境变量可覆盖)。
/// 下层 crate 不在编译期嵌它,`run()` 一进来就装进 `yunxi_base::install_build_id`。
pub const BUILD_ID: &str = env!("YUNXI_BUILD_ID");

pub async fn run() -> Result<()> {
    // 在任何路径、配置或子进程初始化之前承接旧版 Miyu 环境变量。
    yunxi_base::paths::adopt_legacy_environment();
    yunxi_base::install_build_id(BUILD_ID);
    // 趁二进制还在磁盘上，先把自己的路径记下来。daemon 一跑就是几小时，
    // 期间升级安装包或重新编译都会把这个文件换掉，那之后 `/proc/self/exe`
    // 读出来的是 `".../yunxi (deleted)"`，再想 spawn 自己就 ENOENT 了
    // （长图渲染器、闹钟、知识库索引都靠这条路）。
    yunxi_base::paths::prime_yunxi_executable();
    // 经 hook 在 PATH 之外兜底找到的 yunxi(没配 brew shellenv 的 shell),子进程照样
    // 按 PATH 找 rg/chafa——把自己所在的 bin 目录补到 PATH 末尾(09-23 macOS 真机)。
    // current_thread 运行时:这里还没有别的线程,改环境变量是安全的。
    if let Ok(executable) = yunxi_base::paths::yunxi_executable() {
        yunxi_base::paths::extend_path_with_executable_dir(&executable);
    }
    // daemon 不坐在任何 herdr pane 里，得忘掉拉起它的那个 pane 的坐标：不然它起
    // 的中转线 CLI（agy / claude / codex 装着 herdr 钩子）会拿着坐标去认领那个
    // pane，herdr 从此丢掉 YunXi 的上报，侧栏卡在「进行中」（用户 09-23）。同上，
    // 这里还没有别的线程；再往下日志一初始化就有写线程了。
    if std::env::args_os()
        .nth(1)
        .is_some_and(|argument| argument == "__daemon")
    {
        yunxi_base::terminal::herdr::forget_pane_coordinates();
    }
    if yunxi_hosts::platforms::plugins::renderer_worker_requested() {
        return yunxi_hosts::platforms::plugins::run_renderer_worker().await;
    }
    if yunxi_base::embedding::embedding_worker_requested() {
        return yunxi_base::embedding::run_embedding_worker().await;
    }
    let paths = yunxi_base::paths::YunXiPaths::new()?;
    let language = yunxi_base::config::AppConfig::display_language_hint(&paths);
    yunxi_base::i18n::init(language.as_deref().unwrap_or("auto"));
    let cli = cli::parse();
    cli::run(cli, paths).await
}

/// 退出码:`main.rs` 用,见 `cli::exit_code`。
pub fn exit_code_for(error: &anyhow::Error) -> i32 {
    cli::exit_code::exit_code_for(error)
}

/// 错误前缀的本地化文案。`main.rs` 打印失败时要用，而 `i18n` 是私有模块。
pub fn error_label() -> &'static str {
    yunxi_base::i18n::text("error", "错误")
}
