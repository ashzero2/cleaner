#![expect(
    dead_code,
    reason = "the FSEvents watcher is the next live-update layer"
)]

//! A thin, owning wrapper over one per-device `FSEvents` stream.
//!
//! The stream delivers events on its own dispatch queue; they are forwarded over a
//! channel so the rest of the crate never runs inside the callback.

use std::ffi::{CStr, OsStr, OsString, c_char, c_void};
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::ptr::NonNull;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::Duration;

use dispatch2::{DispatchQueue, DispatchQueueAttr, DispatchRetained};
use objc2_core_foundation::{CFArray, CFString};
use objc2_core_services::{
    ConstFSEventStreamRef, FSEventStreamContext, FSEventStreamCreateRelativeToDevice,
    FSEventStreamEventFlags, FSEventStreamEventId, FSEventStreamFlushSync, FSEventStreamInvalidate,
    FSEventStreamRef, FSEventStreamRelease, FSEventStreamSetDispatchQueue, FSEventStreamStart,
    FSEventStreamStop, FSEventsCopyUUIDForDevice, FSEventsGetCurrentEventId,
    kFSEventStreamCreateFlagNoDefer, kFSEventStreamCreateFlagWatchRoot,
};

pub(crate) const MUST_SCAN_SUBDIRS: u32 = 0x01;
pub(crate) const USER_DROPPED: u32 = 0x02;
pub(crate) const KERNEL_DROPPED: u32 = 0x04;
pub(crate) const IDS_WRAPPED: u32 = 0x08;
pub(crate) const HISTORY_DONE: u32 = 0x10;
pub(crate) const ROOT_CHANGED: u32 = 0x20;
pub(crate) const MOUNT: u32 = 0x40;
pub(crate) const UNMOUNT: u32 = 0x80;

#[derive(Debug)]
pub(crate) struct Event {
    pub(crate) id: u64,
    pub(crate) flags: u32,
    pub(crate) path: OsString,
}

#[derive(Debug)]
pub(crate) struct Stream {
    source: FSEventStreamRef,
    queue: DispatchRetained<DispatchQueue>,
    sender: *mut Sender<Event>,
    events: Receiver<Event>,
}

impl Stream {
    /// Starts a stream over `relative` (a path below the volume root) on device `dev`,
    /// replaying history after `since` before switching to live events.
    pub(crate) fn start(
        dev: libc::dev_t,
        relative: &OsStr,
        since: u64,
        latency: Duration,
    ) -> io::Result<Self> {
        let relative = relative
            .to_str()
            .ok_or_else(|| io::Error::from(io::ErrorKind::InvalidInput))?;
        let paths = CFArray::from_retained_objects(&[CFString::from_str(relative)]);
        let (sender, events) = mpsc::channel();
        let sender = Box::into_raw(Box::new(sender));
        let mut context = FSEventStreamContext {
            version: 0,
            info: sender.cast(),
            retain: None,
            release: None,
            copyDescription: None,
        };
        let flags = kFSEventStreamCreateFlagNoDefer | kFSEventStreamCreateFlagWatchRoot;

        // SAFETY: `context` and `paths` are valid for the call (FSEvents copies both),
        // `callback` matches the expected signature, and `sender` outlives the stream.
        let stream = unsafe {
            FSEventStreamCreateRelativeToDevice(
                None,
                Some(callback),
                &raw mut context,
                dev,
                paths.as_opaque(),
                since,
                latency.as_secs_f64(),
                flags,
            )
        };
        let fail = |message: &str| {
            // SAFETY: `sender` came from `Box::into_raw` above and no stream references it.
            drop(unsafe { Box::from_raw(sender) });
            io::Error::other(message.to_owned())
        };
        if stream.is_null() {
            return Err(fail("cannot create FSEvents stream"));
        }

        let queue = DispatchQueue::new("clrs.fsevents", DispatchQueueAttr::SERIAL);
        // SAFETY: `stream` is a valid stream that has not been started yet.
        unsafe { FSEventStreamSetDispatchQueue(stream, Some(&queue)) };
        // SAFETY: `stream` is valid and scheduled on `queue`.
        if !unsafe { FSEventStreamStart(stream) } {
            // SAFETY: the stream never started, so invalidating and releasing it is sound.
            unsafe {
                FSEventStreamInvalidate(stream);
                FSEventStreamRelease(stream);
            }
            return Err(fail("cannot start FSEvents stream"));
        }
        Ok(Self {
            source: stream,
            queue,
            sender,
            events,
        })
    }

    pub(crate) fn recv(&self, timeout: Duration) -> Option<Event> {
        self.events.recv_timeout(timeout).ok()
    }

    pub(crate) fn try_recv(&self) -> Option<Event> {
        self.events.try_recv().ok()
    }

    /// Asks the daemon for every event it has buffered and waits until they have all
    /// been handed to this process.
    pub(crate) fn flush(&self) {
        // SAFETY: `self.stream` is a valid, started stream.
        unsafe { FSEventStreamFlushSync(self.source) };
        self.queue.exec_sync(|| {});
    }
}

impl Drop for Stream {
    fn drop(&mut self) {
        // SAFETY: the stream is valid and released exactly once, in the documented
        // stop, invalidate, release order.
        unsafe {
            FSEventStreamStop(self.source);
            FSEventStreamInvalidate(self.source);
            FSEventStreamRelease(self.source);
        }
        self.queue.exec_sync(|| {});
        // SAFETY: the stream is gone and the queue is drained, so nothing can still be
        // using `sender`, which came from `Box::into_raw` in `start`.
        drop(unsafe { Box::from_raw(self.sender) });
    }
}

unsafe extern "C-unwind" fn callback(
    _stream: ConstFSEventStreamRef,
    info: *mut c_void,
    count: usize,
    paths: NonNull<c_void>,
    flags: NonNull<FSEventStreamEventFlags>,
    ids: NonNull<FSEventStreamEventId>,
) {
    // SAFETY: `info` is the `Sender` registered in `Stream::start`, alive until the
    // stream is torn down.
    let sender = unsafe { &*info.cast::<Sender<Event>>() };
    let paths = paths.as_ptr().cast::<*const c_char>();
    for i in 0..count {
        // SAFETY: FSEvents passes `count` valid, NUL-terminated paths and parallel
        // arrays of `count` flags and ids.
        let (path, flags, id) = unsafe {
            (
                CStr::from_ptr(*paths.add(i)),
                *flags.as_ptr().add(i),
                *ids.as_ptr().add(i),
            )
        };
        let event = Event {
            id,
            flags,
            path: OsStr::from_bytes(path.to_bytes()).to_owned(),
        };
        if sender.send(event).is_err() {
            return;
        }
    }
}

/// The identity of the `FSEvents` database for `dev`; it changes if the history is reset.
pub(crate) fn device_uuid(dev: libc::dev_t) -> Option<[u8; 16]> {
    // SAFETY: plain query with no pointer arguments.
    let uuid = unsafe { FSEventsCopyUUIDForDevice(dev) }?;
    let b = uuid.uuid_bytes();
    Some([
        b.byte0, b.byte1, b.byte2, b.byte3, b.byte4, b.byte5, b.byte6, b.byte7, b.byte8, b.byte9,
        b.byte10, b.byte11, b.byte12, b.byte13, b.byte14, b.byte15,
    ])
}

pub(crate) fn current_event_id() -> u64 {
    // SAFETY: plain query with no arguments.
    unsafe { FSEventsGetCurrentEventId() }
}
