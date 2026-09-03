//! MMCSS registration for the audio threads (t30 / L1).
//!
//! Without this, every capture/render thread is an ordinary `std::thread` at
//! normal scheduler priority. When a game starts it saturates every core *and*
//! registers its own threads with MMCSS, so ours get preempted for tens of
//! milliseconds — long enough to drain the device buffer and crackle. Joining the
//! "Pro Audio" MMCSS task tells the Multimedia Class Scheduler that this thread
//! has a real-time deadline, which is exactly what it is.
//!
//! Best-effort by design: if MMCSS is unavailable the thread keeps running at
//! normal priority, precisely as it did before. Audio must never fail to start
//! because a scheduling hint could not be applied.

#[cfg(target_os = "windows")]
mod platform {
    use tracing::{debug, warn};
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::System::Threading::{
        AvRevertMmThreadCharacteristics, AvSetMmThreadCharacteristicsW, AvSetMmThreadPriority,
        AVRT_PRIORITY_CRITICAL,
    };

    /// Keeps the calling thread in an MMCSS task for as long as it is alive.
    /// Registration is per-thread, so this must be created *on* the audio thread,
    /// not handed to it.
    pub struct MmcssGuard {
        handle: HANDLE,
    }

    impl MmcssGuard {
        /// Join the "Pro Audio" MMCSS task at critical priority, falling back to
        /// the plain "Audio" class. `None` means MMCSS refused — the caller keeps
        /// working at normal priority.
        pub fn pro_audio() -> Option<Self> {
            // Escape hatch for A/B measurement: with NODUS_NO_MMCSS=1 the audio
            // threads stay at normal priority exactly as they were before t30, so
            // the glitch counters can be read against a real baseline instead of
            // being compared to a memory of how bad it used to sound.
            if std::env::var_os("NODUS_NO_MMCSS").is_some() {
                warn!("NODUS_NO_MMCSS set — audio thread stays at normal priority (baseline mode)");
                return None;
            }
            for task in ["Pro Audio", "Audio"] {
                let wide: Vec<u16> = task.encode_utf16().chain(std::iter::once(0)).collect();
                // MMCSS hands back a task index we don't need, but the parameter
                // is not optional.
                let mut task_index: u32 = 0;
                let handle =
                    unsafe { AvSetMmThreadCharacteristicsW(PCWSTR(wide.as_ptr()), &mut task_index) };
                match handle {
                    Ok(h) if !h.is_invalid() => {
                        if let Err(e) = unsafe { AvSetMmThreadPriority(h, AVRT_PRIORITY_CRITICAL) } {
                            // Joined the task but stayed at its default priority —
                            // still far better than nothing.
                            debug!("MMCSS '{task}': critical priority refused: {e}");
                        }
                        debug!("audio thread joined MMCSS task '{task}'");
                        return Some(Self { handle: h });
                    }
                    Ok(_) => continue,
                    Err(e) => {
                        debug!("MMCSS task '{task}' unavailable: {e}");
                        continue;
                    }
                }
            }
            warn!(
                "MMCSS unavailable — audio thread runs at normal priority; \
                 expect dropouts while the system is under heavy load"
            );
            None
        }
    }

    impl Drop for MmcssGuard {
        fn drop(&mut self) {
            unsafe {
                let _ = AvRevertMmThreadCharacteristics(self.handle);
            }
        }
    }
}

#[cfg(not(target_os = "windows"))]
mod platform {
    /// No-op stand-in: MMCSS is a Windows scheduler facility.
    pub struct MmcssGuard;

    impl MmcssGuard {
        pub fn pro_audio() -> Option<Self> {
            None
        }
    }
}

pub use platform::MmcssGuard;
