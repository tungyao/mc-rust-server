//! resources: Tick/在线数/连接通道
use bevy_ecs::prelude::*;
use crossbeam_channel::Receiver;
use std::sync::{Arc, atomic::AtomicUsize};

#[derive(Resource, Default)]
pub struct TickCounter(pub u64);

#[derive(Resource, Clone, Debug, Default)]
pub struct PlayersOnline(pub Arc<AtomicUsize>);

#[derive(Resource)]
pub struct NetReceiver(pub Receiver<mc_network::NewConnection>);
