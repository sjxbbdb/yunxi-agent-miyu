//! 脚本工具的测试。

mod bundled;
mod dashboard;
mod header;
mod host_access;
mod index;
mod manage;
mod run;

use std::path::Path;
use yunxi_base::config::AppConfig;
use yunxi_base::paths::YunXiPaths;

/// 隔离的路径集:用户层在 `<root>/data/scripts`(persona 子层
/// `personas/default`),内置层在 `<root>/system`。
pub(super) fn test_env(root: &Path) -> (AppConfig, YunXiPaths) {
    let mut paths = YunXiPaths::new().unwrap();
    paths.scripts_dir = root.join("data/scripts");
    paths.system_scripts_dir = root.join("system");
    paths.cache_dir = root.join("cache");
    std::fs::create_dir_all(&paths.scripts_dir).unwrap();
    (AppConfig::default(), paths)
}

pub(super) fn write_script(dir: &Path, name: &str, body: &str) -> std::path::PathBuf {
    std::fs::create_dir_all(dir).unwrap();
    let path = dir.join(name);
    std::fs::write(&path, body).unwrap();
    path
}
