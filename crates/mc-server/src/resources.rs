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

/// Play 期事件通道（net 线程 → ECS：移动包/KeepAlive 回显）。
#[derive(Resource)]
pub struct PlayEventReceiver(pub Receiver<mc_network::PlayEvent>);

/// P52 方案 B：ECS 拥有的 socket 运行时（accept 登记 / 任务句柄 / 每连接写队列）。
/// 只包装外部 crate 的类型以满足 Bevy 0.14 的 `Resource` trait，不改变所有权语义。
#[derive(Resource)]
pub struct SocketEcs(pub mc_network::SocketEcsRuntime);
