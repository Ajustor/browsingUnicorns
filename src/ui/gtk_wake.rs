//! GTK's loop on Linux, which the web views run on, only advances when the
//! window pumps it (each frame). Instead of a frame every 16 ms to pump it,
//! a thread waits on what GTK waits on (its file descriptors and its next
//! timer, as GLib's own loop does) and wakes the window when GTK has work.

use std::io::{Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::net::UnixStream;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant};

use glib_sys::GPollFD;

/// At most one wake-up this often (GTK busy: a frame per screen refresh,
/// as before).
const MIN_GAP: Duration = Duration::from_millis(16);

/// What GTK waits on after a frame: its file descriptors, and its next
/// timer (`None`: none).
struct Watch {
    fds: Vec<GPollFD>,
    timeout: Option<Duration>,
}

pub struct GtkWake {
    /// `None` once it goes.
    tx: Option<Sender<Watch>>,
    /// Interrupts the thread's wait: a newer watch is there.
    nudge: UnixStream,
}

impl GtkWake {
    /// Starts the thread that wakes `ctx`; `None` if it can't.
    pub fn start(ctx: egui::Context) -> Option<Self> {
        let (nudge, nudged) = UnixStream::pair().ok()?;
        nudge.set_nonblocking(true).ok()?;
        nudged.set_nonblocking(true).ok()?;
        let (tx, rx) = mpsc::channel();
        std::thread::Builder::new()
            .name("gtk-wake".into())
            .spawn(move || wait_loop(&rx, nudged, &ctx))
            .ok()?;
        Some(Self {
            tx: Some(tx),
            nudge,
        })
    }

    /// Call once GTK was pumped, at the end of a frame: hands what GTK now
    /// waits on to the thread. False when it can't be known (the window
    /// must then pump GTK on its own).
    pub fn watch(&self) -> bool {
        let Some(watch) = query() else {
            return false;
        };
        if self.tx.as_ref().is_none_or(|tx| tx.send(watch).is_err()) {
            return false;
        }
        // Full: the thread is being nudged already.
        let _ = (&self.nudge).write(&[1]);
        true
    }
}

impl Drop for GtkWake {
    /// The thread ends: told by the nudge that nothing comes any more.
    fn drop(&mut self) {
        drop(self.tx.take());
        let _ = (&self.nudge).write(&[1]);
    }
}

/// What GTK's default main context waits on now.
fn query() -> Option<Watch> {
    // SAFETY: GLib's documented protocol for running a context from another
    // loop, on the default context, from the thread that iterates it (the
    // UI thread): acquire, prepare, query, release. `fds` has room for the
    // `len` items `g_main_context_query` is told of.
    unsafe {
        let context = glib_sys::g_main_context_default();
        if glib_sys::g_main_context_acquire(context) == glib_sys::GFALSE {
            return None;
        }
        let mut priority = 0;
        glib_sys::g_main_context_prepare(context, &mut priority);
        let empty = GPollFD {
            fd: 0,
            events: 0,
            revents: 0,
        };
        let mut fds = vec![empty; 16];
        let mut timeout = -1;
        loop {
            let len = fds.len() as i32;
            let n = glib_sys::g_main_context_query(
                context,
                priority,
                &mut timeout,
                fds.as_mut_ptr(),
                len,
            );
            if n <= len {
                fds.truncate(n.max(0) as usize);
                break;
            }
            fds.resize(n as usize, empty);
        }
        glib_sys::g_main_context_release(context);
        Some(Watch {
            fds,
            timeout: u64::try_from(timeout).ok().map(Duration::from_millis),
        })
    }
}

fn wait_loop(rx: &Receiver<Watch>, mut nudged: UnixStream, ctx: &egui::Context) {
    let mut last_wake = Instant::now();
    let Ok(mut watch) = rx.recv() else {
        return;
    };
    loop {
        // The latest frame's.
        while let Ok(newer) = rx.try_recv() {
            watch = newer;
        }
        // GTK's descriptors, then the nudge's.
        let mut fds = watch.fds.clone();
        fds.push(GPollFD {
            fd: nudged.as_raw_fd(),
            events: glib_sys::G_IO_IN as u16,
            revents: 0,
        });
        let wait = watch
            .timeout
            .map_or(-1, |t| i32::try_from(t.as_millis()).unwrap_or(i32::MAX));
        // SAFETY: `fds` is a live array of `fds.len()` GPollFD; g_poll only
        // writes their `revents`. Its result is not needed: a timer due, a
        // descriptor ready or an error (an interrupted wait) all wake the
        // window, a nudge alone doesn't.
        unsafe { glib_sys::g_poll(fds.as_mut_ptr(), fds.len() as u32, wait) };
        let gtk_ready = fds[..fds.len() - 1].iter().any(|f| f.revents != 0);
        if fds[fds.len() - 1].revents != 0 {
            let mut drain = [0u8; 64];
            while matches!(nudged.read(&mut drain), Ok(n) if n > 0) {}
            match rx.try_recv() {
                Ok(newer) => watch = newer,
                // The window is gone.
                Err(mpsc::TryRecvError::Disconnected) => return,
                Err(mpsc::TryRecvError::Empty) => {}
            }
            if !gtk_ready {
                // Only a newer watch: wait on it.
                continue;
            }
        }
        if let Some(rest) = MIN_GAP.checked_sub(last_wake.elapsed()) {
            std::thread::sleep(rest);
        }
        last_wake = Instant::now();
        ctx.request_repaint();
        // Until the frame says what GTK waits on next.
        match rx.recv() {
            Ok(newer) => watch = newer,
            Err(_) => return,
        }
    }
}
