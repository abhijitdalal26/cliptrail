//! `tracing` → logcat (tag "ClipTrail"). Without this, Rust logs on
//! Android go nowhere. Our log lines never contain clip content.

use std::ffi::{CString, c_char, c_int};
use std::io::{self, Write};

use tracing_subscriber::EnvFilter;
use tracing_subscriber::fmt::MakeWriter;

#[link(name = "log")]
unsafe extern "C" {
    fn __android_log_write(prio: c_int, tag: *const c_char, text: *const c_char) -> c_int;
}

const ANDROID_LOG_INFO: c_int = 4;

struct Logcat;

/// Buffers one formatted event and writes it as one logcat line on drop.
struct Line(Vec<u8>);

impl Write for Line {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Drop for Line {
    fn drop(&mut self) {
        let text = String::from_utf8_lossy(&self.0);
        let text = text.trim_end();
        if text.is_empty() {
            return;
        }
        if let Ok(c) = CString::new(text.replace('\0', " ")) {
            // SAFETY: both pointers are valid NUL-terminated strings for the call.
            unsafe {
                __android_log_write(ANDROID_LOG_INFO, c"ClipTrail".as_ptr(), c.as_ptr());
            }
        }
    }
}

impl<'a> MakeWriter<'a> for Logcat {
    type Writer = Line;
    fn make_writer(&'a self) -> Line {
        Line(Vec::new())
    }
}

/// Install the subscriber (once per process; later calls are no-ops).
pub fn init() {
    let _ = tracing_subscriber::fmt()
        .with_writer(Logcat)
        .with_ansi(false)
        .without_time()
        .with_target(false)
        .with_env_filter(EnvFilter::new(
            "info,cliptrail_desktop_lib=info,clipd_sync=info",
        ))
        .try_init();
}
