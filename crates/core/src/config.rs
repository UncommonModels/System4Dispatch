//! Configuring other nodes: finding the nodes on the bus, reading their
//! CDI, and reading and writing their configuration memory with the
//! *Memory Configuration* protocol over datagrams.
//!
//! [`NodeConfig`] is part of the [`Dispatcher`](crate::Dispatcher), which
//! feeds it the messages it receives and sends what it queues; use it
//! through [`Dispatcher::config`](crate::Dispatcher::config) and the
//! dispatcher's `discover_nodes`, `load_cdi`, `read_config` and
//! `write_config` methods.
//!
//! # Protocol
//!
//! One memory operation is in flight at a time; the rest wait in a queue.
//! Each is a datagram to the target node:
//!
//! ```text
//!  read:  20 4s AA AA AA AA [space] count     → reply 20 5s AA AA AA AA [space] data…
//!  write: 20 0s AA AA AA AA [space] data…     → reply 20 1s AA AA AA AA [space]   (optional)
//! ```
//!
//! `s` is 1, 2 or 3 for spaces `0xFD`, `0xFE` and `0xFF`, with no space
//! byte; any other space uses `s` = 0 and puts the space in byte 6. Failed
//! replies (`0x58`/`0x18`) carry a 2-byte error code after the address.
//!
//! The target answers our datagram with Datagram Received OK (if its flags
//! have bit 7 set, a reply datagram follows within 2^(flags & 0x0F)
//! seconds) or Datagram Rejected (codes with bit `0x2000` set mean "try
//! again"). Every datagram addressed to us is acknowledged with Datagram
//! Received OK.
//!
//! The CDI (space `0xFF`) is read 64 bytes at a time until a chunk contains
//! a NUL, comes back short, or the read fails past the start.

use std::collections::{BTreeMap, VecDeque};

use rolcb::message::Message;
use rolcb::{NodeId, NodeInfo};

use crate::cdi::Cdi;

/// The datagram content type for Memory Configuration.
pub const DATAGRAM_MEMORY_CONFIG: u8 = 0x20;
/// The CDI memory space.
pub const SPACE_CDI: u8 = 0xFF;
/// The main configuration memory space.
pub const SPACE_CONFIG: u8 = 0xFD;
/// Most bytes one read or write datagram carries.
pub const MAX_CHUNK: usize = 64;
/// How long to wait for a datagram to be acknowledged.
pub const ACK_TIMEOUT_MS: u64 = 3_000;
/// How long to wait for a reply when the target doesn't say.
pub const REPLY_TIMEOUT_MS: u64 = 3_000;
const MAX_RESENDS: u8 = 3;

const CMD_WRITE: u8 = 0x00;
const CMD_WRITE_REPLY: u8 = 0x10;
const CMD_WRITE_FAILED: u8 = 0x18;
const CMD_READ: u8 = 0x40;
const CMD_READ_REPLY: u8 = 0x50;
const CMD_READ_FAILED: u8 = 0x58;
const CMD_UPDATE_COMPLETE: u8 = 0xA8;
const CMD_RESET: u8 = 0xA9;

/// The command-byte flags and optional space byte for `space`.
fn space_bits(space: u8) -> (u8, Option<u8>) {
    match space {
        0xFD..=0xFF => (space & 0x03, None),
        _ => (0, Some(space)),
    }
}

fn header(cmd: u8, space: u8, addr: u32) -> Vec<u8> {
    let (bits, byte) = space_bits(space);
    let mut d = vec![DATAGRAM_MEMORY_CONFIG, cmd | bits];
    d.extend_from_slice(&addr.to_be_bytes());
    d.extend(byte);
    d
}

/// A read command datagram.
pub fn read_command(space: u8, addr: u32, count: u8) -> Vec<u8> {
    let mut d = header(CMD_READ, space, addr);
    d.push(count);
    d
}

/// A write command datagram.
pub fn write_command(space: u8, addr: u32, data: &[u8]) -> Vec<u8> {
    let mut d = header(CMD_WRITE, space, addr);
    d.extend_from_slice(data);
    d
}

/// A decoded read or write reply.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MemReply {
    /// Data read.
    Read {
        /// Memory space.
        space: u8,
        /// Address of the first byte.
        addr: u32,
        /// The bytes.
        data: Vec<u8>,
    },
    /// A read failed.
    ReadFailed {
        /// Memory space.
        space: u8,
        /// Address asked for.
        addr: u32,
        /// OpenLCB error code.
        code: u16,
    },
    /// A write completed.
    Written {
        /// Memory space.
        space: u8,
        /// Address written.
        addr: u32,
    },
    /// A write failed.
    WriteFailed {
        /// Memory space.
        space: u8,
        /// Address written.
        addr: u32,
        /// OpenLCB error code.
        code: u16,
    },
}

/// Decode a Memory Configuration reply datagram.
pub fn decode_reply(d: &[u8]) -> Option<MemReply> {
    if d.len() < 6 || d[0] != DATAGRAM_MEMORY_CONFIG {
        return None;
    }
    let cmd = d[1] & 0xFC;
    let bits = d[1] & 0x03;
    let addr = u32::from_be_bytes([d[2], d[3], d[4], d[5]]);
    let (space, rest) = if bits == 0 {
        (*d.get(6)?, &d[7..])
    } else {
        (0xFC | bits, &d[6..])
    };
    let code = || {
        rest.get(..2)
            .map(|c| u16::from_be_bytes([c[0], c[1]]))
            .unwrap_or(0)
    };
    Some(match cmd {
        CMD_READ_REPLY => MemReply::Read {
            space,
            addr,
            data: rest.to_vec(),
        },
        CMD_READ_FAILED => MemReply::ReadFailed {
            space,
            addr,
            code: code(),
        },
        CMD_WRITE_REPLY => MemReply::Written { space, addr },
        CMD_WRITE_FAILED => MemReply::WriteFailed {
            space,
            addr,
            code: code(),
        },
        _ => return None,
    })
}

/// A node seen on the bus.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct RemoteNode {
    /// Current CAN alias.
    pub alias: u16,
    /// Simple Node Information, once it has answered.
    pub info: Option<NodeInfo>,
    /// Protocol Support flags, once it has answered.
    pub protocols: Option<u32>,
}

impl RemoteNode {
    /// A name for lists: the user name, else manufacturer and model.
    pub fn label(&self, id: NodeId) -> String {
        match &self.info {
            Some(i) if !i.user_name.is_empty() => i.user_name.clone(),
            Some(i) if !i.model.is_empty() => format!("{} {}", i.manufacturer, i.model),
            _ => id.to_string(),
        }
    }

    /// Whether the node says it has a CDI (true if it hasn't said).
    pub fn has_cdi(&self) -> bool {
        self.protocols
            .is_none_or(|p| p & rolcb::message::protocol::CDI != 0)
    }
}

/// What is known about one node's configuration.
#[derive(Debug, Clone, Default)]
pub struct ConfigData {
    /// CDI bytes read so far.
    pub cdi_raw: Vec<u8>,
    /// The parsed CDI, once read: `Err` holds why it couldn't be parsed.
    pub cdi: Option<Result<Cdi, String>>,
    /// Configuration bytes read or written, by (space, address).
    pub memory: BTreeMap<(u8, u32), u8>,
    /// The last error for this node, if any.
    pub error: Option<String>,
}

impl ConfigData {
    /// `len` bytes from `addr`, if all of them are known.
    pub fn bytes(&self, space: u8, addr: u32, len: u32) -> Option<Vec<u8>> {
        (0..len)
            .map(|i| self.memory.get(&(space, addr + i)).copied())
            .collect()
    }
}

#[derive(Debug, Clone, PartialEq)]
enum OpKind {
    ReadCdi,
    Read { space: u8, addr: u32, len: u8 },
    Write { space: u8, addr: u32, data: Vec<u8> },
    UpdateComplete,
    Reset,
}

#[derive(Debug, Clone)]
struct Op {
    node: NodeId,
    kind: OpKind,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Stage {
    AwaitAck,
    AwaitReply,
}

#[derive(Debug, Clone)]
struct InFlight {
    op: Op,
    alias: u16,
    datagram: Vec<u8>,
    stage: Stage,
    deadline: u64,
    resends: u8,
}

/// Node discovery and memory configuration state. See [the module docs](self).
#[derive(Debug, Default)]
pub struct NodeConfig {
    /// Nodes seen on the bus.
    pub nodes: BTreeMap<NodeId, RemoteNode>,
    /// Configuration data by node.
    pub data: BTreeMap<NodeId, ConfigData>,
    queue: VecDeque<Op>,
    current: Option<InFlight>,
    outbox: Vec<Message>,
}

impl NodeConfig {
    /// Operations waiting or in flight for `node`.
    pub fn busy(&self, node: NodeId) -> usize {
        self.queue.iter().filter(|o| o.node == node).count()
            + self.current.as_ref().is_some_and(|c| c.op.node == node) as usize
    }

    /// Operations waiting or in flight for any node.
    pub fn busy_total(&self) -> usize {
        self.queue.len() + self.current.is_some() as usize
    }

    /// When the dispatcher next needs to poll for a timeout.
    pub fn next_deadline(&self) -> Option<u64> {
        self.current.as_ref().map(|c| c.deadline)
    }

    pub(crate) fn take_outgoing(&mut self) -> Vec<Message> {
        std::mem::take(&mut self.outbox)
    }

    /// Forget all aliases and abandon queued work (the link went down).
    pub(crate) fn link_down(&mut self) {
        self.queue.clear();
        self.current = None;
        for n in self.nodes.values_mut() {
            n.alias = 0;
        }
    }

    /// Ask every node to identify itself, and the nodes we know to send
    /// their names again (they may have been renamed).
    pub(crate) fn discover(&mut self) {
        self.outbox
            .push(Message::VerifyNodeIdGlobal { node_id: None });
        for n in self.nodes.values().filter(|n| n.alias != 0) {
            self.outbox.push(Message::SnipRequest { dest: n.alias });
        }
    }

    /// A node announced itself or answered Verify Node ID.
    pub(crate) fn saw_node(&mut self, id: NodeId, alias: u16) {
        let new = !self.nodes.contains_key(&id);
        let n = self.nodes.entry(id).or_default();
        n.alias = alias;
        if new || n.info.is_none() {
            self.outbox.push(Message::SnipRequest { dest: alias });
            self.outbox
                .push(Message::ProtocolSupportInquiry { dest: alias });
        }
    }

    fn node_by_alias(&mut self, alias: u16) -> Option<&mut RemoteNode> {
        self.nodes
            .values_mut()
            .find(|n| n.alias == alias && alias != 0)
    }

    pub(crate) fn enqueue_cdi(&mut self, node: NodeId, now: u64) {
        let d = self.data.entry(node).or_default();
        d.cdi_raw.clear();
        d.cdi = None;
        d.error = None;
        self.queue.push_back(Op {
            node,
            kind: OpKind::ReadCdi,
        });
        self.start_next(now);
    }

    pub(crate) fn enqueue_read(&mut self, node: NodeId, space: u8, addr: u32, len: u32, now: u64) {
        self.data.entry(node).or_default().error = None;
        let mut a = addr;
        let end = addr + len;
        while a < end {
            let n = (end - a).min(MAX_CHUNK as u32);
            self.queue.push_back(Op {
                node,
                kind: OpKind::Read {
                    space,
                    addr: a,
                    len: n as u8,
                },
            });
            a += n;
        }
        self.start_next(now);
    }

    pub(crate) fn enqueue_write(
        &mut self,
        node: NodeId,
        space: u8,
        addr: u32,
        data: &[u8],
        now: u64,
    ) {
        self.data.entry(node).or_default().error = None;
        for (i, chunk) in data.chunks(MAX_CHUNK).enumerate() {
            self.queue.push_back(Op {
                node,
                kind: OpKind::Write {
                    space,
                    addr: addr + (i * MAX_CHUNK) as u32,
                    data: chunk.to_vec(),
                },
            });
        }
        self.start_next(now);
    }

    pub(crate) fn enqueue_update_complete(&mut self, node: NodeId, now: u64) {
        self.queue.push_back(Op {
            node,
            kind: OpKind::UpdateComplete,
        });
        self.start_next(now);
    }

    pub(crate) fn enqueue_reset(&mut self, node: NodeId, now: u64) {
        self.queue.push_back(Op {
            node,
            kind: OpKind::Reset,
        });
        self.start_next(now);
    }

    fn fail(&mut self, node: NodeId, why: String) {
        self.data.entry(node).or_default().error = Some(why);
        // Don't run the rest of a failed job.
        self.queue.retain(|o| o.node != node);
    }

    fn start_next(&mut self, now: u64) {
        while self.current.is_none() {
            let Some(op) = self.queue.pop_front() else {
                return;
            };
            let alias = self.nodes.get(&op.node).map(|n| n.alias).unwrap_or(0);
            if alias == 0 {
                self.fail(op.node, "node is not on the bus".into());
                continue;
            }
            let datagram = match &op.kind {
                OpKind::ReadCdi => {
                    let at = self.data.get(&op.node).map_or(0, |d| d.cdi_raw.len()) as u32;
                    read_command(SPACE_CDI, at, MAX_CHUNK as u8)
                }
                OpKind::Read { space, addr, len } => read_command(*space, *addr, *len),
                OpKind::Write { space, addr, data } => write_command(*space, *addr, data),
                OpKind::UpdateComplete => vec![DATAGRAM_MEMORY_CONFIG, CMD_UPDATE_COMPLETE],
                OpKind::Reset => vec![DATAGRAM_MEMORY_CONFIG, CMD_RESET],
            };
            self.outbox.push(Message::Datagram {
                dest: alias,
                data: datagram.clone(),
            });
            self.current = Some(InFlight {
                op,
                alias,
                datagram,
                stage: Stage::AwaitAck,
                deadline: now + ACK_TIMEOUT_MS,
                resends: 0,
            });
        }
    }

    /// Run timeouts.
    pub(crate) fn poll(&mut self, now: u64) {
        let Some(c) = &self.current else { return };
        if now < c.deadline {
            return;
        }
        let c = self.current.take().unwrap();
        // A reset node often never answers; that's success.
        if c.op.kind != OpKind::Reset {
            let what = match c.stage {
                Stage::AwaitAck => "did not acknowledge",
                Stage::AwaitReply => "did not reply",
            };
            self.fail(c.op.node, format!("node {what} in time"));
        }
        self.start_next(now);
    }

    /// Handle a received message. `me` is our alias. Returns true if
    /// anything visible changed.
    pub(crate) fn receive(&mut self, src: u16, msg: &Message, me: u16, now: u64) -> bool {
        match msg {
            Message::InitializationComplete { node_id, .. }
            | Message::VerifiedNodeId { node_id, .. } => {
                self.saw_node(*node_id, src);
                true
            }
            Message::SnipReply { dest, data } if *dest == me => {
                let info = NodeInfo::from_snip(data);
                if let Some(n) = self.node_by_alias(src) {
                    n.info = Some(info);
                }
                true
            }
            Message::ProtocolSupportReply { dest, flags } if *dest == me => {
                if let Some(n) = self.node_by_alias(src) {
                    n.protocols = Some(*flags);
                }
                true
            }
            Message::DatagramReceivedOk { dest, flags } if *dest == me => {
                let Some(c) = self.current.as_mut().filter(|c| c.alias == src) else {
                    return false;
                };
                if c.stage != Stage::AwaitAck {
                    return false;
                }
                let expects_reply = match c.op.kind {
                    OpKind::ReadCdi | OpKind::Read { .. } => true,
                    OpKind::Write { .. } => flags & 0x80 != 0,
                    OpKind::UpdateComplete | OpKind::Reset => false,
                };
                if expects_reply {
                    c.stage = Stage::AwaitReply;
                    let exp = (flags & 0x0F) as u32;
                    let wait = if flags & 0x80 != 0 && exp > 0 {
                        (1u64 << exp.min(16)) * 1_000
                    } else {
                        REPLY_TIMEOUT_MS
                    };
                    c.deadline = now + wait.max(REPLY_TIMEOUT_MS);
                    false
                } else {
                    self.finish_write(now)
                }
            }
            Message::DatagramRejected { dest, code } if *dest == me => {
                let Some(c) = self.current.as_mut().filter(|c| c.alias == src) else {
                    return false;
                };
                if code & 0x2000 != 0 && c.resends < MAX_RESENDS {
                    c.resends += 1;
                    c.deadline = now + ACK_TIMEOUT_MS;
                    let d = Message::Datagram {
                        dest: src,
                        data: c.datagram.clone(),
                    };
                    self.outbox.push(d);
                    return false;
                }
                let c = self.current.take().unwrap();
                self.fail(
                    c.op.node,
                    format!("node rejected the request (error {code:04X})"),
                );
                self.start_next(now);
                true
            }
            Message::Datagram { dest, data } if *dest == me => {
                self.outbox.push(Message::DatagramReceivedOk {
                    dest: src,
                    flags: 0,
                });
                match decode_reply(data) {
                    Some(r) => self.reply(src, r, now),
                    None => false,
                }
            }
            _ => false,
        }
    }

    fn finish_write(&mut self, now: u64) -> bool {
        let Some(c) = self.current.take() else {
            return false;
        };
        if let OpKind::Write { space, addr, data } = &c.op.kind {
            let d = self.data.entry(c.op.node).or_default();
            for (i, b) in data.iter().enumerate() {
                d.memory.insert((*space, addr + i as u32), *b);
            }
            // The name or description changed: fetch what SNIP now says.
            if *space == crate::cdi::SPACE_ACDI_USER {
                self.outbox.push(Message::SnipRequest { dest: c.alias });
            }
        }
        self.start_next(now);
        true
    }

    fn reply(&mut self, src: u16, r: MemReply, now: u64) -> bool {
        let Some(c) = self.current.as_ref().filter(|c| c.alias == src) else {
            return false;
        };
        let node = c.op.node;
        match (&c.op.kind, r) {
            (
                OpKind::ReadCdi,
                MemReply::Read {
                    space: SPACE_CDI,
                    data,
                    ..
                },
            ) => {
                let d = self.data.entry(node).or_default();
                let end = data.iter().position(|b| *b == 0);
                let short = data.len() < MAX_CHUNK;
                d.cdi_raw
                    .extend_from_slice(&data[..end.unwrap_or(data.len())]);
                self.current = None;
                if end.is_some() || short || data.is_empty() {
                    self.parse_cdi(node);
                } else {
                    // Next chunk, ahead of anything else queued.
                    self.queue.push_front(Op {
                        node,
                        kind: OpKind::ReadCdi,
                    });
                }
            }
            (OpKind::ReadCdi, MemReply::ReadFailed { code, .. }) => {
                self.current = None;
                if self.data.get(&node).is_some_and(|d| !d.cdi_raw.is_empty()) {
                    // Ran off the end of the space.
                    self.parse_cdi(node);
                } else {
                    self.fail(node, format!("node has no readable CDI (error {code:04X})"));
                }
            }
            (
                OpKind::Read {
                    space: s, addr: a, ..
                },
                MemReply::Read { space, addr, data },
            ) if *s == space && *a == addr => {
                let d = self.data.entry(node).or_default();
                for (i, b) in data.iter().enumerate() {
                    d.memory.insert((space, addr + i as u32), *b);
                }
                self.current = None;
            }
            (OpKind::Read { .. }, MemReply::ReadFailed { space, addr, code }) => {
                self.current = None;
                self.fail(
                    node,
                    format!("reading space {space:02X} at {addr} failed (error {code:04X})"),
                );
            }
            (OpKind::Write { .. }, MemReply::Written { .. }) => return self.finish_write(now),
            (OpKind::Write { .. }, MemReply::WriteFailed { space, addr, code }) => {
                self.current = None;
                self.fail(
                    node,
                    format!("writing space {space:02X} at {addr} failed (error {code:04X})"),
                );
            }
            _ => return false,
        }
        self.start_next(now);
        true
    }

    fn parse_cdi(&mut self, node: NodeId) {
        let d = self.data.entry(node).or_default();
        let text = String::from_utf8_lossy(&d.cdi_raw).into_owned();
        d.cdi = Some(Cdi::parse(&text));
    }
}

/// Group reads of many fields into as few datagrams as possible: runs of
/// (space, address, size) at most [`MAX_CHUNK`] bytes long, bridging gaps
/// of up to 16 bytes between fields.
pub fn plan_reads(mut fields: Vec<(u8, u32, u32)>) -> Vec<(u8, u32, u32)> {
    fields.sort();
    let mut out: Vec<(u8, u32, u32)> = Vec::new();
    for (space, addr, size) in fields {
        if size == 0 {
            continue;
        }
        if let Some(last) = out.last_mut() {
            let end = last.1 + last.2;
            let new_end = (addr + size).max(end);
            if last.0 == space && addr <= end + 16 && new_end - last.1 <= MAX_CHUNK as u32 {
                last.2 = new_end - last.1;
                continue;
            }
        }
        out.push((space, addr, size));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commands_match_the_standard() {
        assert_eq!(
            read_command(0xFF, 0x40, 64),
            [0x20, 0x43, 0, 0, 0, 0x40, 64]
        );
        assert_eq!(read_command(0xFD, 1, 8), [0x20, 0x41, 0, 0, 0, 1, 8]);
        assert_eq!(
            read_command(0xFB, 1, 63),
            [0x20, 0x40, 0, 0, 0, 1, 0xFB, 63]
        );
        assert_eq!(write_command(0xFD, 2, &[9]), [0x20, 0x01, 0, 0, 0, 2, 9]);
        assert_eq!(
            write_command(0xFB, 2, &[9]),
            [0x20, 0x00, 0, 0, 0, 2, 0xFB, 9]
        );
    }

    #[test]
    fn replies_decode() {
        assert_eq!(
            decode_reply(&[0x20, 0x53, 0, 0, 0, 0x40, b'<', b'c']),
            Some(MemReply::Read {
                space: 0xFF,
                addr: 0x40,
                data: b"<c".to_vec()
            })
        );
        assert_eq!(
            decode_reply(&[0x20, 0x50, 0, 0, 0, 1, 0xFB, b'x']),
            Some(MemReply::Read {
                space: 0xFB,
                addr: 1,
                data: b"x".to_vec()
            })
        );
        assert_eq!(
            decode_reply(&[0x20, 0x59, 0, 0, 1, 0, 0x10, 0x82]),
            Some(MemReply::ReadFailed {
                space: 0xFD,
                addr: 256,
                code: 0x1082
            })
        );
        assert_eq!(
            decode_reply(&[0x20, 0x11, 0, 0, 0, 4]),
            Some(MemReply::Written {
                space: 0xFD,
                addr: 4
            })
        );
        assert_eq!(decode_reply(&[0x30, 0x50, 0, 0, 0, 0]), None);
    }

    #[test]
    fn reads_are_batched() {
        let plan = plan_reads(vec![
            (0xFD, 0, 1),
            (0xFD, 1, 8),
            (0xFD, 20, 32),
            (0xFD, 100, 8),
            (0xFB, 1, 63),
            (0xFD, 52, 40),
        ]);
        assert_eq!(plan, vec![(0xFB, 1, 63), (0xFD, 0, 52), (0xFD, 52, 56)]);
    }
}
