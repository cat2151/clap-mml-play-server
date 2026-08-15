use std::{
    ffi::OsStr,
    io::Cursor,
    sync::atomic::{AtomicBool, Ordering},
};

use super::*;

#[test]
fn guard_requires_an_explicit_one() {
    assert!(!guard_is_enabled(None));
    assert!(!guard_is_enabled(Some(OsStr::new("0"))));
    assert!(!guard_is_enabled(Some(OsStr::new("true"))));
    assert!(guard_is_enabled(Some(OsStr::new("1"))));
}

#[test]
fn stdin_eof_requests_shutdown() {
    let shutdown = AtomicBool::new(false);

    monitor_until_closed(Cursor::new(b"ignored input"), &shutdown);

    assert!(shutdown.load(Ordering::SeqCst));
}
