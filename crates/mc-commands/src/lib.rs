//! mc-commands: Brigadier-like 命令图，抄 FerrumC graph
use std::collections::HashMap;

#[derive(Debug, Default)]
pub struct CommandGraph {
    pub commands: HashMap<String, String>,
}

impl CommandGraph {
    pub fn register(&mut self, name: &str, desc: &str) {
        self.commands.insert(name.to_string(), desc.to_string());
    }
}
