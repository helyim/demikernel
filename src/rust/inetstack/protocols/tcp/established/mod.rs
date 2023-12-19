// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

mod background;
pub mod congestion_control;
mod ctrlblk;
mod rto;
mod sender;

use crate::{
    collections::async_queue::SharedAsyncQueue,
    inetstack::{
        protocols::{
            ipv4::Ipv4Header,
            tcp::{
                congestion_control::CongestionControlConstructor,
                established::ctrlblk::SharedControlBlock,
                segment::TcpHeader,
                SeqNumber,
            },
        },
        MacAddress,
        SharedArpPeer,
        TcpConfig,
    },
    runtime::{
        fail::Fail,
        memory::DemiBuffer,
        network::NetworkRuntime,
        scheduler::{
            TaskHandle,
            Yielder,
        },
        QDesc,
        SharedBox,
        SharedDemiRuntime,
    },
};
use ::futures::channel::mpsc;
use ::std::{
    net::SocketAddrV4,
    time::Duration,
};

#[derive(Clone)]
pub struct EstablishedSocket {
    pub cb: SharedControlBlock,
    recv_queue: SharedAsyncQueue<(Ipv4Header, TcpHeader, DemiBuffer)>,
    // We need this to eventually stop the background task on close.
    #[allow(unused)]
    runtime: SharedDemiRuntime,
    /// The background co-routines handles various tasks, such as retransmission and acknowledging.
    /// We annotate it as unused because the compiler believes that it is never called which is not the case.
    #[allow(unused)]
    background: TaskHandle,
}

impl EstablishedSocket {
    pub fn new(
        local: SocketAddrV4,
        remote: SocketAddrV4,
        mut runtime: SharedDemiRuntime,
        transport: SharedBox<dyn NetworkRuntime>,
        recv_queue: SharedAsyncQueue<(Ipv4Header, TcpHeader, DemiBuffer)>,
        ack_queue: SharedAsyncQueue<usize>,
        local_link_addr: MacAddress,
        tcp_config: TcpConfig,
        arp: SharedArpPeer,
        receiver_seq_no: SeqNumber,
        ack_delay_timeout: Duration,
        receiver_window_size: u32,
        receiver_window_scale: u32,
        sender_seq_no: SeqNumber,
        sender_window_size: u32,
        sender_window_scale: u8,
        sender_mss: usize,
        cc_constructor: CongestionControlConstructor,
        congestion_control_options: Option<congestion_control::Options>,
        dead_socket_tx: mpsc::UnboundedSender<QDesc>,
    ) -> Result<Self, Fail> {
        // TODO: Maybe add the queue descriptor here.
        let cb = SharedControlBlock::new(
            local,
            remote,
            runtime.clone(),
            transport,
            local_link_addr,
            tcp_config,
            arp,
            receiver_seq_no,
            ack_delay_timeout,
            receiver_window_size,
            receiver_window_scale,
            sender_seq_no,
            sender_window_size,
            sender_window_scale,
            sender_mss,
            cc_constructor,
            congestion_control_options,
            recv_queue.clone(),
            ack_queue.clone(),
        );
        let handle: TaskHandle = runtime.insert_background_coroutine(
            "Inetstack::TCP::established::background",
            Box::pin(background::background(cb.clone(), dead_socket_tx)),
        )?;
        Ok(Self {
            cb,
            recv_queue,
            background: handle.clone(),
            runtime: runtime.clone(),
        })
    }

    pub fn get_recv_queue(&self) -> SharedAsyncQueue<(Ipv4Header, TcpHeader, DemiBuffer)> {
        self.recv_queue.clone()
    }

    pub fn send(&mut self, buf: DemiBuffer) -> Result<(), Fail> {
        self.cb.send(buf)
    }

    pub async fn push(&mut self, nbytes: usize, yielder: Yielder) -> Result<(), Fail> {
        self.cb.push(nbytes, yielder).await
    }

    pub async fn pop(&mut self, size: Option<usize>, yielder: Yielder) -> Result<DemiBuffer, Fail> {
        self.cb.pop(size, yielder).await
    }

    pub async fn close(&mut self, yielder: Yielder) -> Result<(), Fail> {
        self.cb.close(yielder).await
    }

    pub fn remote_mss(&self) -> usize {
        self.cb.remote_mss()
    }

    pub fn current_rto(&self) -> Duration {
        self.cb.rto()
    }

    pub fn endpoints(&self) -> (SocketAddrV4, SocketAddrV4) {
        (self.cb.get_local(), self.cb.get_remote())
    }
}

//======================================================================================================================
// Trait Implementations
//======================================================================================================================

// TODO: Uncomment this once we have proper resource clean up on asynchronous close.
// FIXME: https://github.com/microsoft/demikernel/issues/988
// impl Drop for EstablishedSocket {
//     fn drop(&mut self) {
//         if let Err(e) = self.runtime.remove_background_coroutine(&self.background) {
//             panic!("Failed to drop established socket (error={})", e);
//         }
//     }
// }
