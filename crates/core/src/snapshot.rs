//! On-disk form of a scanned [`Tree`].
//!
//! Layout (all integers little-endian): magic and version, scan parameters, the volume
//! and journal position the scan corresponds to, unreadable paths, then one record per
//! live block in breadth-first order, and a CRC-32 of everything before it.
//!
//! Loading never trusts the file: every length and index is checked before use, blocks
//! must form a tree (each referenced once, only by an earlier block), and the checksum
//! must match, so a damaged or hostile file is rejected rather than causing a panic.

use std::collections::{HashSet, VecDeque};
use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufReader, BufWriter, Read, Write};
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crc32fast::Hasher;

use crate::error::Error;
use crate::sys::EntryKind;
use crate::tree::{Block, FLAG_MULTI, Link, NO_BLOCK, Node, NodeId, Tree};

const MAGIC: &[u8; 8] = b"CLRSNAP\0";
const VERSION: u32 = 2;
const NODE_BYTES: usize = 24;
const LINK_BYTES: usize = 32;
const MAX_PATH_BYTES: usize = 16 * 1024;
const BUFFER: usize = 1 << 20;
const STALE_TEMP: std::time::Duration = std::time::Duration::from_secs(3600);

#[derive(Debug)]
pub(crate) struct Meta {
    pub(crate) root: PathBuf,
    pub(crate) cross_devices: bool,
    pub(crate) uuid: [u8; 16],
    pub(crate) event_id: u64,
    pub(crate) saved_at: u64,
    pub(crate) full_scan_at: u64,
    pub(crate) issues: Vec<(PathBuf, i32)>,
}

pub(crate) fn save(path: &Path, tree: &Tree, meta: &Meta) -> Result<(), Error> {
    let io_error = |source| Error::CacheIo {
        path: path.to_path_buf(),
        source,
    };
    let dir = path
        .parent()
        .ok_or_else(|| io_error(io::ErrorKind::InvalidInput.into()))?;
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(dir)
        .map_err(io_error)?;

    let temp = path.with_extension(format!("tmp-{}", std::process::id()));
    let written = write_file(&temp, tree, meta).and_then(|()| fs::rename(&temp, path));
    if written.is_err() {
        fs::remove_file(&temp).ok();
    }
    written.map_err(io_error)
}

/// Removes leftovers of interrupted saves (temp files older than an hour).
pub(crate) fn sweep_stale_temp_files(snapshot: &Path) {
    let Some(dir) = snapshot.parent() else {
        return;
    };
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let stale = path
            .extension()
            .is_some_and(|ext| ext.as_bytes().starts_with(b"tmp-"))
            && entry
                .metadata()
                .and_then(|m| m.modified())
                .ok()
                .and_then(|modified| modified.elapsed().ok())
                .is_some_and(|age| age > STALE_TEMP);
        if stale {
            fs::remove_file(path).ok();
        }
    }
}

fn write_file(path: &Path, tree: &Tree, meta: &Meta) -> io::Result<()> {
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)?;
    let mut out = Writer {
        inner: BufWriter::with_capacity(BUFFER, file),
        crc: Hasher::new(),
    };

    let (order, remap) = live_blocks(tree);
    out.put(MAGIC)?;
    out.u32(VERSION)?;
    out.u32(u32::from(meta.cross_devices))?;
    out.put(&meta.uuid)?;
    out.u64(meta.event_id)?;
    out.u64(meta.saved_at)?;
    out.u64(meta.full_scan_at)?;
    out.bytes(meta.root.as_os_str().as_bytes())?;
    out.u32(len_u32(meta.issues.len())?)?;
    for (issue, errno) in &meta.issues {
        out.bytes(issue.as_os_str().as_bytes())?;
        out.put(&errno.to_le_bytes())?;
    }

    out.u32(len_u32(order.len())?)?;
    let mut scratch = Vec::new();
    for &id in &order {
        let block = &tree.blocks[id as usize];
        out.u32(len_u32(block.names.len())?)?;
        out.u32(len_u32(block.nodes.len())?)?;
        out.u32(len_u32(block.links.len())?)?;
        out.put(&block.names)?;

        scratch.clear();
        for node in &block.nodes {
            let child = if node.block == NO_BLOCK {
                NO_BLOCK
            } else {
                remap[node.block as usize]
            };
            scratch.extend_from_slice(&node.size.to_le_bytes());
            scratch.extend_from_slice(&node.name_start.to_le_bytes());
            scratch.extend_from_slice(&node.files.to_le_bytes());
            scratch.extend_from_slice(&child.to_le_bytes());
            scratch.extend_from_slice(&node.name_len.to_le_bytes());
            scratch.push(node.kind.code());
            scratch.push(node.flags & FLAG_MULTI);
        }
        for link in &block.links {
            scratch.extend_from_slice(&link.name_start.to_le_bytes());
            scratch.extend_from_slice(&link.dev.to_le_bytes());
            scratch.extend_from_slice(&link.ino.to_le_bytes());
            scratch.extend_from_slice(&link.alloc.to_le_bytes());
            scratch.extend_from_slice(&link.nlink.to_le_bytes());
            scratch.extend_from_slice(&[0; 4]);
        }
        out.put(&scratch)?;
    }

    let checksum = out.crc.finalize();
    out.inner.write_all(&checksum.to_le_bytes())?;
    let file = out
        .inner
        .into_inner()
        .map_err(io::IntoInnerError::into_error)?;
    file.sync_all()
}

/// Blocks reachable from the root in breadth-first order, and the old-to-new id map.
fn live_blocks(tree: &Tree) -> (Vec<u32>, Vec<u32>) {
    let mut remap = vec![NO_BLOCK; tree.blocks.len()];
    let mut order = Vec::new();
    let mut queue = VecDeque::from([0u32]);
    while let Some(id) = queue.pop_front() {
        remap[id as usize] = u32::try_from(order.len()).unwrap_or(NO_BLOCK);
        order.push(id);
        queue.extend(
            tree.blocks[id as usize]
                .nodes
                .iter()
                .filter(|node| node.kind == EntryKind::Dir && node.block != NO_BLOCK)
                .map(|node| node.block),
        );
    }
    (order, remap)
}

fn len_u32(len: usize) -> io::Result<u32> {
    u32::try_from(len).map_err(|_| io::Error::from(io::ErrorKind::InvalidInput))
}

struct Writer<W: Write> {
    inner: W,
    crc: Hasher,
}

impl<W: Write> Writer<W> {
    fn put(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.crc.update(bytes);
        self.inner.write_all(bytes)
    }

    fn u32(&mut self, value: u32) -> io::Result<()> {
        self.put(&value.to_le_bytes())
    }

    fn u64(&mut self, value: u64) -> io::Result<()> {
        self.put(&value.to_le_bytes())
    }

    fn bytes(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.u32(len_u32(bytes.len())?)?;
        self.put(bytes)
    }
}

pub(crate) fn load(path: &Path) -> Result<(Tree, Meta), Error> {
    let file = File::open(path).map_err(|source| Error::CacheIo {
        path: path.to_path_buf(),
        source,
    })?;
    let length = file
        .metadata()
        .map_err(|source| Error::CacheIo {
            path: path.to_path_buf(),
            source,
        })?
        .len();
    let mut reader = Reader {
        inner: BufReader::with_capacity(BUFFER, file),
        crc: Hasher::new(),
        remaining: length.checked_sub(4).ok_or(corrupt("file too short"))?,
    };
    parse(&mut reader)
}

fn corrupt(reason: &'static str) -> Error {
    Error::CorruptSnapshot(reason)
}

fn parse(reader: &mut Reader<BufReader<File>>) -> Result<(Tree, Meta), Error> {
    if reader.array::<8>()? != *MAGIC {
        return Err(corrupt("not a snapshot"));
    }
    if reader.u32()? != VERSION {
        return Err(corrupt("unsupported version"));
    }
    let cross_devices = reader.u32()? != 0;
    let uuid = reader.array::<16>()?;
    let event_id = reader.u64()?;
    let saved_at = reader.u64()?;
    let full_scan_at = reader.u64()?;
    let root = path_from(reader.bytes(MAX_PATH_BYTES)?);

    let issue_count = reader.u32()? as usize;
    let mut issues = Vec::with_capacity(issue_count.min(4096));
    for _ in 0..issue_count {
        let path = path_from(reader.bytes(MAX_PATH_BYTES)?);
        issues.push((path, i32::from_le_bytes(reader.array()?)));
    }

    let block_count = reader.u32()? as usize;
    if block_count == 0 || block_count >= NO_BLOCK as usize {
        return Err(corrupt("bad block count"));
    }
    let mut blocks = Vec::with_capacity(block_count.min(1 << 20));
    let mut parents: Vec<Option<NodeId>> = vec![None; block_count];
    let mut referenced = vec![false; block_count];
    let mut total = 0usize;
    for index in 0..block_count {
        let block = read_block(reader, index, block_count, &mut parents, &mut referenced)?;
        total += block.nodes.len();
        blocks.push(Arc::new(block));
    }

    if blocks[0].nodes.len() != 1 || blocks[0].nodes[0].kind != EntryKind::Dir {
        return Err(corrupt("bad root block"));
    }
    if referenced.iter().skip(1).any(|&seen| !seen) {
        return Err(corrupt("unreachable block"));
    }

    let expected = reader.crc.clone().finalize();
    if reader.remaining != 0 {
        return Err(corrupt("trailing data"));
    }
    let mut trailer = [0u8; 4];
    reader
        .inner
        .read_exact(&mut trailer)
        .map_err(|_| corrupt("missing checksum"))?;
    if u32::from_le_bytes(trailer) != expected {
        return Err(corrupt("checksum mismatch"));
    }

    let tree = Tree {
        root_path: root.clone(),
        blocks,
        parents,
        len: total,
    };
    let meta = Meta {
        root,
        cross_devices,
        uuid,
        event_id,
        saved_at,
        full_scan_at,
        issues,
    };
    Ok((tree, meta))
}

fn read_block(
    reader: &mut Reader<BufReader<File>>,
    index: usize,
    block_count: usize,
    parents: &mut [Option<NodeId>],
    referenced: &mut [bool],
) -> Result<Block, Error> {
    let names_len = reader.u32()? as usize;
    let nodes_len = reader.u32()? as usize;
    let links_len = reader.u32()? as usize;
    let names = reader.vec(names_len)?.into_boxed_slice();
    let node_bytes = reader.vec(
        nodes_len
            .checked_mul(NODE_BYTES)
            .ok_or(corrupt("node count"))?,
    )?;
    let link_bytes = reader.vec(
        links_len
            .checked_mul(LINK_BYTES)
            .ok_or(corrupt("link count"))?,
    )?;

    let mut nodes = Vec::with_capacity(nodes_len);
    for (position, raw) in node_bytes.chunks_exact(NODE_BYTES).enumerate() {
        let node = Node {
            size: u64::from_le_bytes(raw[0..8].try_into().map_err(|_| corrupt("node"))?),
            name_start: u32::from_le_bytes(raw[8..12].try_into().map_err(|_| corrupt("node"))?),
            files: u32::from_le_bytes(raw[12..16].try_into().map_err(|_| corrupt("node"))?),
            block: u32::from_le_bytes(raw[16..20].try_into().map_err(|_| corrupt("node"))?),
            name_len: u16::from_le_bytes(raw[20..22].try_into().map_err(|_| corrupt("node"))?),
            kind: EntryKind::from_code(raw[22]).ok_or(corrupt("bad entry kind"))?,
            flags: raw[23],
        };
        if node.flags & !FLAG_MULTI != 0 {
            return Err(corrupt("bad node flags"));
        }
        if u64::from(node.name_start) + u64::from(node.name_len) > names_len as u64 {
            return Err(corrupt("name out of range"));
        }
        if node.block != NO_BLOCK {
            let child = node.block as usize;
            if node.kind != EntryKind::Dir
                || child <= index
                || child >= block_count
                || referenced[child]
            {
                return Err(corrupt("bad block reference"));
            }
            referenced[child] = true;
            parents[child] = Some(NodeId::new(
                u32::try_from(index).map_err(|_| corrupt("block index"))?,
                u32::try_from(position).map_err(|_| corrupt("node index"))?,
            ));
        }
        nodes.push(node);
    }

    let linked: HashSet<u32> = if links_len == 0 {
        HashSet::new()
    } else {
        nodes
            .iter()
            .filter(|node| node.flags & FLAG_MULTI != 0)
            .map(|node| node.name_start)
            .collect()
    };
    let mut links = Vec::with_capacity(links_len);
    for raw in link_bytes.chunks_exact(LINK_BYTES) {
        let link = Link {
            name_start: u32::from_le_bytes(raw[0..4].try_into().map_err(|_| corrupt("link"))?),
            dev: i32::from_le_bytes(raw[4..8].try_into().map_err(|_| corrupt("link"))?),
            ino: u64::from_le_bytes(raw[8..16].try_into().map_err(|_| corrupt("link"))?),
            alloc: u64::from_le_bytes(raw[16..24].try_into().map_err(|_| corrupt("link"))?),
            nlink: u32::from_le_bytes(raw[24..28].try_into().map_err(|_| corrupt("link"))?),
        };
        if !linked.contains(&link.name_start) {
            return Err(corrupt("link without node"));
        }
        links.push(link);
    }
    Ok(Block {
        names,
        nodes: nodes.into_boxed_slice(),
        links: links.into_boxed_slice(),
    })
}

fn path_from(bytes: Vec<u8>) -> PathBuf {
    PathBuf::from(OsString::from_vec(bytes))
}

struct Reader<R: Read> {
    inner: R,
    crc: Hasher,
    remaining: u64,
}

impl<R: Read> Reader<R> {
    fn fill(&mut self, buf: &mut [u8]) -> Result<(), Error> {
        let wanted = buf.len() as u64;
        if wanted > self.remaining {
            return Err(corrupt("unexpected end of file"));
        }
        self.inner
            .read_exact(buf)
            .map_err(|_| corrupt("unexpected end of file"))?;
        self.crc.update(buf);
        self.remaining -= wanted;
        Ok(())
    }

    fn array<const N: usize>(&mut self) -> Result<[u8; N], Error> {
        let mut buf = [0u8; N];
        self.fill(&mut buf)?;
        Ok(buf)
    }

    fn u32(&mut self) -> Result<u32, Error> {
        self.array().map(u32::from_le_bytes)
    }

    fn u64(&mut self) -> Result<u64, Error> {
        self.array().map(u64::from_le_bytes)
    }

    fn vec(&mut self, len: usize) -> Result<Vec<u8>, Error> {
        if len as u64 > self.remaining {
            return Err(corrupt("length exceeds file"));
        }
        let mut buf = vec![0u8; len];
        self.fill(&mut buf)?;
        Ok(buf)
    }

    fn bytes(&mut self, limit: usize) -> Result<Vec<u8>, Error> {
        let len = self.u32()? as usize;
        if len > limit {
            return Err(corrupt("string too long"));
        }
        self.vec(len)
    }
}
