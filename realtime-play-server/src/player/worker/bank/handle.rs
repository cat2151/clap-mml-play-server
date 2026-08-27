//! coordinator が持つ bank worker のハンドル。
//!
//! 「送る／受け取る」と「いま返事待ちか」だけを持つ。仕事の中身（音色ロードか
//! render か）は [`super::BankWorkers`] の側にあり、ここは知らない。
//!
//! # 返事待ちは 1 bank につき 1 つ
//! 返事は 1 本の channel に相乗りしているので、返事を受け取る前に次の要求を送ると
//! 「どの要求の返事か」が分からなくなる。[`BankWorker::busy`] がその 1 つを守る。
//! coordinator は先読み中の bank へ render を送らないことでこの制約を満たす。

use std::{
    cell::Cell,
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc::{Receiver, Sender},
        Arc,
    },
    thread::JoinHandle,
};

use anyhow::anyhow;
use cmrt_core::{RealtimeRenderer, RendererHandoff};

use super::super::super::instances::LiveInstancesSpec;
use super::protocol::{BankCommand, BankReply};
use super::state::run_bank_worker;

pub(super) struct BankWorker {
    bank: usize,
    commands: Sender<BankCommand>,
    replies: Receiver<BankReply>,
    join: Option<JoinHandle<()>>,
    /// 送信済みで返事を受け取っていない仕事があるか。
    busy: Cell<bool>,
    /// 塞がっているせいで render を出せなかった instance の延べ数。
    /// **これが増えるのは契約違反**（鳴っている bank へ先読みを送った）だけ。
    render_skips: Cell<u64>,
    /// この bank が render したブロック数。worker スレッドが進める。
    blocks: Arc<AtomicU64>,
}

impl BankWorker {
    pub(super) fn spawn(
        bank: usize,
        renderers: Vec<RealtimeRenderer>,
        spec: LiveInstancesSpec,
    ) -> Self {
        let (command_tx, command_rx) = std::sync::mpsc::channel();
        let (reply_tx, reply_rx) = std::sync::mpsc::channel();
        let handoff = renderers
            .into_iter()
            .map(RendererHandoff::new)
            .collect::<Vec<_>>();
        let blocks = Arc::new(AtomicU64::new(0));
        let worker_blocks = Arc::clone(&blocks);
        let join = std::thread::Builder::new()
            .name(format!("realtime-play-server-bank{bank}"))
            .spawn(move || {
                run_bank_worker(bank, handoff, spec, worker_blocks, &command_rx, &reply_tx);
            })
            .inspect_err(|error| {
                // 立たなければこの bank の instance は一切鳴らない。要求は
                // 「worker が居ない」エラーとして表面化する。
                eprintln!("cmrt-bank-worker: bank={bank} event=spawn-failed detail={error}");
            })
            .ok();
        Self {
            bank,
            commands: command_tx,
            replies: reply_rx,
            join,
            busy: Cell::new(false),
            render_skips: Cell::new(0),
            blocks,
        }
    }

    pub(super) fn bank(&self) -> usize {
        self.bank
    }

    /// この bank が今までに render したブロック数。**演奏中に読んでよい。**
    pub(super) fn blocks(&self) -> u64 {
        self.blocks.load(Ordering::Relaxed)
    }

    pub(super) fn busy(&self) -> bool {
        self.busy.get()
    }

    /// 塞がっていて render を出せなかった instance を 1 つ数える。
    pub(super) fn note_render_skip(&self) {
        self.render_skips.set(self.render_skips.get() + 1);
    }

    pub(super) fn render_skips(&self) -> u64 {
        self.render_skips.get()
    }

    /// 返事の要る仕事を送る。受け取りは [`Self::receive`] / [`Self::try_receive`]。
    pub(super) fn send(&self, command: BankCommand) -> anyhow::Result<()> {
        if self.busy.get() {
            // 起きない想定（coordinator が先読み中の bank へ他の仕事を出さない）。
            // 起きたら channel の対応が 1 つずれるので、古い返事を捨てて揃え直す。
            eprintln!(
                "cmrt-bank-worker: bank={} event=reply-discarded reason=overlapping-request",
                self.bank
            );
            let _ = self.receive();
        }
        self.commands
            .send(command)
            .map_err(|_| anyhow!("bank {} worker is gone", self.bank))?;
        self.busy.set(true);
        Ok(())
    }

    /// 返事を待つ。
    pub(super) fn receive(&self) -> anyhow::Result<BankReply> {
        let reply = self
            .replies
            .recv()
            .map_err(|_| anyhow!("bank {} worker exited while working", self.bank));
        self.busy.set(false);
        reply
    }

    /// 返事が来ていれば取り出す。来ていなければ `None`（待たない）。
    pub(super) fn try_receive(&self) -> Option<anyhow::Result<BankReply>> {
        match self.replies.try_recv() {
            Ok(reply) => {
                self.busy.set(false);
                Some(Ok(reply))
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => None,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.busy.set(false);
                Some(Err(anyhow!(
                    "bank {} worker exited while working",
                    self.bank
                )))
            }
        }
    }

    /// 送って、その返事だけを待つ。
    pub(super) fn request(&self, command: BankCommand) -> anyhow::Result<BankReply> {
        self.send(command)?;
        self.receive()
    }

    /// 返事の要らない仕事。
    pub(super) fn notify(&self, command: BankCommand) {
        let _ = self.commands.send(command);
    }

    pub(super) fn take_join(&mut self) -> Option<JoinHandle<()>> {
        self.join.take()
    }
}
