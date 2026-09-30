//! 工具按回合所属的账号开会话库。

use anyhow::Result;
use yunxi_base::config::AppConfig;
use yunxi_base::paths::YunXiPaths;
use yunxi_core::state::StateStore;

/// 成员的会话在他自己的库里（`home/<用户>/conversation.db`）。挂进管理员库时
/// 外键对不上，会报 `FOREIGN KEY constraint failed`（goal 工具踩过）。口径同
/// kb_root_for / artifacts_root：`member_home_dir()` 是 Some，就开那家的库。
pub(crate) fn store_for(config: &AppConfig, paths: &YunXiPaths) -> Result<StateStore> {
    match config.member_home_dir() {
        Some(home) => StateStore::open_at_home(paths, &home),
        None => StateStore::new(paths),
    }
}
