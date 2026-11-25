//! mc-server lib：只给集成测试（`tests/`）暴露管理面板模块；
//! 二进制入口见 `main.rs`（两边各自编译一份 `admin`，无共享状态）。
pub mod admin;
pub mod admin_items;
