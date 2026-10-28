use std::ffi::{CStr, CString, OsStr};
use std::io;
use std::mem::MaybeUninit;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

pub(crate) mod fsevents;

const ATTR_CMN_ERROR: u32 = 0x2000_0000;
pub(crate) const EPERM: i32 = libc::EPERM;
pub(crate) const EIO: i32 = libc::EIO;
const VREG: u32 = 1;
const VDIR: u32 = 2;
const VLNK: u32 = 5;

const COMMON_ATTRS: u32 = libc::ATTR_CMN_RETURNED_ATTRS
    | ATTR_CMN_ERROR
    | libc::ATTR_CMN_NAME
    | libc::ATTR_CMN_DEVID
    | libc::ATTR_CMN_OBJTYPE
    | libc::ATTR_CMN_FILEID;
const FILE_ATTRS: u32 = libc::ATTR_FILE_LINKCOUNT | libc::ATTR_FILE_ALLOCSIZE;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum EntryKind {
    File,
    Dir,
    Symlink,
    Other,
}

impl EntryKind {
    pub(crate) fn code(self) -> u8 {
        match self {
            Self::File => 0,
            Self::Dir => 1,
            Self::Symlink => 2,
            Self::Other => 3,
        }
    }

    pub(crate) fn from_code(code: u8) -> Option<Self> {
        match code {
            0 => Some(Self::File),
            1 => Some(Self::Dir),
            2 => Some(Self::Symlink),
            3 => Some(Self::Other),
            _ => None,
        }
    }

    fn from_vtype(vtype: u32) -> Self {
        match vtype {
            VREG => Self::File,
            VDIR => Self::Dir,
            VLNK => Self::Symlink,
            _ => Self::Other,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Attrs {
    pub(crate) kind: EntryKind,
    pub(crate) dev: i32,
    pub(crate) inode: u64,
    pub(crate) links: u32,
    pub(crate) allocated: u64,
}

/// Whether `error` means the path no longer exists or is no longer a directory.
pub(crate) fn is_gone(error: &io::Error) -> bool {
    error.kind() == io::ErrorKind::NotFound || error.raw_os_error() == Some(libc::ENOTDIR)
}

/// The mount point of the volume that holds `path`.
pub(crate) fn mount_point(path: &Path) -> io::Result<PathBuf> {
    let path = CString::new(path.as_os_str().as_bytes())
        .map_err(|_| io::Error::from(io::ErrorKind::InvalidInput))?;
    let mut stat = MaybeUninit::<libc::statfs>::uninit();
    // SAFETY: `path` is NUL-terminated and `stat` points to writable memory of the right size.
    if unsafe { libc::statfs(path.as_ptr(), stat.as_mut_ptr()) } < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: `statfs` succeeded, so the struct is fully initialised.
    let stat = unsafe { stat.assume_init() };
    // SAFETY: `f_mntonname` is a NUL-terminated string inside the initialised struct.
    let name = unsafe { CStr::from_ptr(stat.f_mntonname.as_ptr()) };
    Ok(PathBuf::from(OsStr::from_bytes(name.to_bytes())))
}

#[derive(Debug)]
pub(crate) struct DirHandle(OwnedFd);

impl DirHandle {
    pub(crate) fn open(path: &Path) -> io::Result<Self> {
        let path = CString::new(path.as_os_str().as_bytes())
            .map_err(|_| io::Error::from(io::ErrorKind::InvalidInput))?;
        let flags = libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC;
        // SAFETY: `path` is a valid NUL-terminated string that outlives the call.
        let fd = unsafe { libc::open(path.as_ptr(), flags) };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: `fd` was just returned by a successful `open` and has no other owner.
        Ok(Self(unsafe { OwnedFd::from_raw_fd(fd) }))
    }

    /// Reads every entry of the directory, invoking `visit` with the entry name and
    /// either its attributes or the per-entry error reported by the kernel.
    pub(crate) fn read_entries(
        &self,
        buf: &mut [u8],
        mut visit: impl FnMut(&[u8], io::Result<Attrs>),
    ) -> io::Result<()> {
        let mut request = libc::attrlist {
            bitmapcount: libc::ATTR_BIT_MAP_COUNT,
            reserved: 0,
            commonattr: COMMON_ATTRS,
            volattr: 0,
            dirattr: 0,
            fileattr: FILE_ATTRS,
            forkattr: 0,
        };
        loop {
            // SAFETY: the descriptor is an open directory, and `request` and `buf` are
            // valid for reads and writes of their full sizes for the duration of the call.
            let count = unsafe {
                libc::getattrlistbulk(
                    self.0.as_raw_fd(),
                    (&raw mut request).cast(),
                    buf.as_mut_ptr().cast(),
                    buf.len(),
                    0,
                )
            };
            match usize::try_from(count) {
                Ok(0) => return Ok(()),
                Ok(count) => parse_batch(buf, count, &mut visit)?,
                Err(_) => {
                    let error = io::Error::last_os_error();
                    if error.kind() != io::ErrorKind::Interrupted {
                        return Err(error);
                    }
                }
            }
        }
    }
}

fn parse_batch(
    buf: &[u8],
    count: usize,
    visit: &mut impl FnMut(&[u8], io::Result<Attrs>),
) -> io::Result<()> {
    let mut offset = 0;
    for _ in 0..count {
        let rest = buf.get(offset..).ok_or_else(truncated)?;
        let len = Reader::new(rest).u32()? as usize;
        let record = rest.get(..len).ok_or_else(truncated)?;
        let (name, attrs) = parse_record(record)?;
        visit(name, attrs);
        offset += len;
    }
    Ok(())
}

fn parse_record(record: &[u8]) -> io::Result<(&[u8], io::Result<Attrs>)> {
    let mut reader = Reader::new(record);
    reader.skip(4)?;
    let common = reader.u32()?;
    reader.skip(8)?;
    let file = reader.u32()?;
    reader.skip(4)?;

    let errno = if common & ATTR_CMN_ERROR != 0 {
        reader.u32()?
    } else {
        0
    };
    let name = if common & libc::ATTR_CMN_NAME != 0 {
        let base = reader.pos;
        let offset = reader.i32()?;
        let len = reader.u32()? as usize;
        let start = base
            .checked_add_signed(offset as isize)
            .ok_or_else(truncated)?;
        let raw = record.get(start..start + len).ok_or_else(truncated)?;
        raw.split(|&b| b == 0).next().unwrap_or_default()
    } else {
        &[]
    };

    if errno != 0 {
        let code = i32::try_from(errno).unwrap_or(libc::EIO);
        return Ok((name, Err(io::Error::from_raw_os_error(code))));
    }

    let dev = optional(common & libc::ATTR_CMN_DEVID != 0, || reader.i32())?;
    let vtype = optional(common & libc::ATTR_CMN_OBJTYPE != 0, || reader.u32())?;
    let inode = optional(common & libc::ATTR_CMN_FILEID != 0, || reader.u64())?;
    let links = optional(file & libc::ATTR_FILE_LINKCOUNT != 0, || reader.u32())?;
    let allocated = optional(file & libc::ATTR_FILE_ALLOCSIZE != 0, || reader.i64())?;

    Ok((
        name,
        Ok(Attrs {
            kind: EntryKind::from_vtype(vtype),
            dev,
            inode,
            links: links.max(1),
            allocated: u64::try_from(allocated).unwrap_or(0),
        }),
    ))
}

fn optional<T: Default>(present: bool, read: impl FnOnce() -> io::Result<T>) -> io::Result<T> {
    if present { read() } else { Ok(T::default()) }
}

fn truncated() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "truncated getattrlistbulk record",
    )
}

struct Reader<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn new(buf: &'a [u8]) -> Self {
        Self { buf, pos: 0 }
    }

    fn take<const N: usize>(&mut self) -> io::Result<[u8; N]> {
        let bytes = self
            .buf
            .get(self.pos..)
            .and_then(<[u8]>::first_chunk::<N>)
            .ok_or_else(truncated)?;
        self.pos += N;
        Ok(*bytes)
    }

    fn skip(&mut self, n: usize) -> io::Result<()> {
        if self.buf.len() < self.pos + n {
            return Err(truncated());
        }
        self.pos += n;
        Ok(())
    }

    fn u32(&mut self) -> io::Result<u32> {
        self.take().map(u32::from_ne_bytes)
    }

    fn i32(&mut self) -> io::Result<i32> {
        self.take().map(i32::from_ne_bytes)
    }

    fn u64(&mut self) -> io::Result<u64> {
        self.take().map(u64::from_ne_bytes)
    }

    fn i64(&mut self) -> io::Result<i64> {
        self.take().map(i64::from_ne_bytes)
    }
}
