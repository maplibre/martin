//! What the dashboard is showing, and how.

/// How much of the frame the log pane takes.
///
/// The string serialization is the title the log pane wears in that size.
#[derive(Clone, Copy, Default, PartialEq, Eq, strum::IntoStaticStr)]
pub enum LogSize {
    /// The log sits under the panels.
    #[default]
    #[strum(serialize = " Log   l expand ")]
    Normal,
    /// The log has everything below the header to itself.
    #[strum(serialize = " Log   l shrink ")]
    Expanded,
}

/// How the log pane is being looked at.
#[derive(Clone, Copy, Default)]
pub struct LogView {
    pub size: LogSize,
    /// How many lines above the newest one the pane stops, `0` while it follows the log.
    pub scroll: usize,
}

impl LogView {
    /// Swaps the log between sitting under the panels and having the screen to itself.
    pub(super) fn toggle_size(&mut self) {
        self.size = match self.size {
            LogSize::Normal => LogSize::Expanded,
            LogSize::Expanded => LogSize::Normal,
        };
    }

    /// Moves `lines` towards the oldest of the `len` lines the log holds, stopping at it.
    pub(super) fn scroll_back(&mut self, lines: usize, len: usize) {
        self.scroll = self
            .clamped(len)
            .saturating_add(lines)
            .min(Self::oldest(len));
    }

    /// Moves `lines` back towards the newest line, stopping at it.
    pub(super) fn scroll_forward(&mut self, lines: usize, len: usize) {
        self.scroll = self.clamped(len).saturating_sub(lines);
    }

    /// Stops at the oldest of the `len` lines the log holds.
    pub(super) fn scroll_to_oldest(&mut self, len: usize) {
        self.scroll = Self::oldest(len);
    }

    /// Follows the log again.
    pub(super) fn follow(&mut self) {
        self.scroll = 0;
    }

    fn clamped(self, len: usize) -> usize {
        self.scroll.min(Self::oldest(len))
    }

    fn oldest(len: usize) -> usize {
        len.saturating_sub(1)
    }
}
