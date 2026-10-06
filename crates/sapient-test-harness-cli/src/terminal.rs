//! Restores the terminal's input settings when a `session` ends.
//!
//! The interactive line editor runs `readline` on its own thread, in raw
//! mode, and a blocked `readline` can't be cancelled from outside. If the
//! session ends for another reason (the peer disconnects, a write fails),
//! the process would exit with the terminal still raw: no echo, no line
//! editing in the user's shell. [`TerminalGuard`] saves the settings before
//! the editor starts and puts them back when dropped.

/// Saved stdin terminal settings, restored on drop.
pub(crate) struct TerminalGuard(imp::Saved);

impl TerminalGuard {
    /// Save stdin's current terminal settings, or `None` if stdin isn't a
    /// terminal (or the platform has no settings to save).
    pub(crate) fn save() -> Option<Self> {
        imp::save().map(TerminalGuard)
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        imp::restore(&self.0);
    }
}

#[cfg(unix)]
mod imp {
    pub(super) type Saved = libc::termios;

    pub(super) fn save() -> Option<Saved> {
        let mut termios = std::mem::MaybeUninit::<libc::termios>::uninit();
        // SAFETY: tcgetattr fully initialises `termios` when it returns 0.
        if unsafe { libc::tcgetattr(libc::STDIN_FILENO, termios.as_mut_ptr()) } == 0 {
            Some(unsafe { termios.assume_init() })
        } else {
            None
        }
    }

    pub(super) fn restore(saved: &Saved) {
        // SAFETY: `saved` came from a successful tcgetattr on the same fd.
        unsafe {
            libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, saved);
        }
    }
}

#[cfg(windows)]
mod imp {
    use windows_sys::Win32::System::Console::{
        CONSOLE_MODE, GetConsoleMode, GetStdHandle, STD_INPUT_HANDLE, SetConsoleMode,
    };

    pub(super) type Saved = CONSOLE_MODE;

    pub(super) fn save() -> Option<Saved> {
        let mut mode: CONSOLE_MODE = 0;
        // SAFETY: GetStdHandle has no preconditions; GetConsoleMode only
        // writes `mode`, and fails (returns 0) for a non-console handle.
        let ok = unsafe { GetConsoleMode(GetStdHandle(STD_INPUT_HANDLE), &mut mode) };
        (ok != 0).then_some(mode)
    }

    pub(super) fn restore(saved: &Saved) {
        // SAFETY: as above; SetConsoleMode takes the mode by value.
        unsafe {
            SetConsoleMode(GetStdHandle(STD_INPUT_HANDLE), *saved);
        }
    }
}

#[cfg(not(any(unix, windows)))]
mod imp {
    pub(super) type Saved = ();

    pub(super) fn save() -> Option<Saved> {
        None
    }

    pub(super) fn restore(_: &Saved) {}
}
