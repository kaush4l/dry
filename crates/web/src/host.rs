//! What the browser can reach outside the page: the clock, and nothing
//! else. Its `Host` refuses files and processes with one line saying why
//! (a `file` or `terminal` artifact shows that line in its snapshot and
//! its calls' results), and the clock is the browser's `Date`.

use dry_core::live::{Clock, Content, Host, Job, Proc, Time};

/// The line every refused file call says.
pub const NO_FILES: &str =
    "no files in the browser: a file artifact reads and writes a folder on a computer; open this engine in the desktop app";
/// The line every refused command says.
pub const NO_PROCESSES: &str = "no terminal in the browser: commands run on a computer; open this engine in the desktop app";

/// The browser as a host.
pub struct Browser;

/// Unix seconds now, from `Date.now()`.
pub fn now() -> i64 {
    (js_sys::Date::now() / 1000.0) as i64
}

impl Time for Browser {
    /// Now, with this browser's offset east of UTC (`Date`'s offset is
    /// minutes west).
    fn clock(&self) -> Clock {
        let offset = -(js_sys::Date::new_0().get_timezone_offset() as i32) * 60;
        Clock::new(now(), offset)
    }
}

impl Host for Browser {
    fn now(&self) -> i64 {
        now()
    }
    // A granted folder (`crate::caps`) is the browser's file capability:
    // artifacts rooted at `folder` read and write it.
    fn list(&self, root: &str, max: usize) -> Result<Vec<String>, String> {
        if root == crate::caps::FOLDER_ROOT {
            return crate::caps::folder_list(max);
        }
        Err(NO_FILES.into())
    }
    fn read(&self, root: &str, path: &str) -> Content {
        if root == crate::caps::FOLDER_ROOT {
            return crate::caps::folder_read(path);
        }
        Content::Missing(NO_FILES.into())
    }
    fn write(&self, root: &str, path: &str, text: &str) -> Result<(), String> {
        if root == crate::caps::FOLDER_ROOT {
            return crate::caps::folder_write(path, text);
        }
        Err(NO_FILES.into())
    }
    fn run(&self, _: &Job) -> Result<Option<i32>, String> {
        Err(NO_PROCESSES.into())
    }
    fn spawn(&self, _: &Job) -> Result<(), String> {
        Err(NO_PROCESSES.into())
    }
    fn status(&self, _: &Job) -> Proc {
        Proc::Ended(None)
    }
    fn tail(&self, _: &Job, _: usize) -> String {
        String::new()
    }
}

/// Outside tools (MCP): there are none here. The core asks this first
/// for every call, so it knows none (`None`) and the built-in tools take
/// theirs; a call nothing knows gets the built-ins' "no such tool".
pub fn external(_: &dry_core::calls::Call) -> Option<Result<String, String>> {
    None
}
