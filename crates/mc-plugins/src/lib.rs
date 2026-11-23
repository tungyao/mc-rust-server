//! mc-plugins: Phase1 Rust Plugin (Bevy Plugin), Phase2 WASM (wasmtime)
use bevy_ecs::prelude::*;
use bevy_ecs::schedule::Schedule;

#[derive(Debug, Clone)]
pub struct PluginEvent(pub String);

/// 所有 Rust 原生插件实现这个 trait，再包一层变成 Bevy Plugin
pub trait ServerPlugin: Send + Sync + 'static {
    fn name(&self) -> &'static str;
    fn register_systems(&self, schedule: &mut Schedule);
}

#[derive(Resource, Default)]
pub struct LoadedPlugins(pub Vec<String>);
