//! 引导画面的版式工具与小部件。
//!
//! 2026-09-20 整体搬进 [`yunxi_base::terminal::chrome`]，好让设置界面
//! （`yunxi config`）复用同一套构件；这里只留一层转发，引导各屏的 import
//! 一行都不用改。新的版式构件加在 chrome 里，不要在这儿另起一份。

pub(in crate::oobe) use yunxi_base::terminal::chrome::*;
