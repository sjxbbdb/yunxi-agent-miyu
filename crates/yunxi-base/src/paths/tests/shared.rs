//! 路径测试共用的 fixture。

use crate::paths::*;

pub(super) fn test_layouts(root: &Path) -> (LegacyLayout, Layout) {
    (
        LegacyLayout {
            config_dir: root.join("legacy/config"),
            data_dir: root.join("legacy/data"),
            cache_dir: root.join("legacy/cache"),
            state_dir: root.join("legacy/state"),
            documents_dir: root.join("Documents/YunXi"),
            pictures_dirs: vec![root.join("Pictures/yunxi"), root.join("Pictures/YunXi")],
        },
        Layout {
            root_dir: root.join(".yunxi"),
            config_dir: root.join(".yunxi/config"),
            data_dir: root.join(".yunxi/data"),
            cache_dir: root.join(".yunxi/cache"),
            state_dir: root.join(".yunxi/state"),
        },
    )
}
