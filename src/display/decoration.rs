//! Implements the CSS [Line Decoration] propagation rules.
//!
//! [Line Decoration]: https://drafts.csswg.org/css-text-decor/#line-decoration

use rasterize::color::BGRA8;

use crate::{layout::FixedL, style::ComputedStyle, text::FontMetrics};

pub struct DecorationTracker {
    propagated: Vec<PropagatedDecoration>,
    first_inactive_propagated: usize,
    active: Vec<ActiveDecoration>,
    first_active: usize,
}

struct PropagatedDecoration {
    color: BGRA8,
    kind: DecorationKind,
}

#[derive(Debug, Clone, Copy)]
pub struct ActiveDecoration {
    pub baseline_offset: FixedL,
    pub thickness: FixedL,
    pub color: BGRA8,
    pub kind: DecorationKind,
}

#[derive(Debug, Clone, Copy)]
pub enum DecorationKind {
    Underline,
    LineThrough,
}

impl DecorationTracker {
    pub const fn new() -> Self {
        Self {
            propagated: Vec::new(),
            first_inactive_propagated: 0,
            active: Vec::new(),
            first_active: 0,
        }
    }

    pub fn root(&mut self) -> DecorationContext<'_> {
        DecorationContext {
            tracker: self,
            scope: DecorationScope::Other {
                restore_propagated_len: 0,
            },
        }
    }
}

#[must_use]
pub struct DecorationContext<'c> {
    tracker: &'c mut DecorationTracker,
    scope: DecorationScope,
}

enum DecorationScope {
    Suspend {
        restore_first_active: usize,
    },
    Inline {
        restore_first_inactive_propagated: usize,
        restore_active_len: usize,
    },
    Other {
        restore_propagated_len: usize,
    },
}

impl DecorationContext<'_> {
    pub fn active_decorations(&self) -> &[ActiveDecoration] {
        &self.tracker.active[self.tracker.first_active..]
    }

    fn push_active_decoration(
        active: &mut Vec<ActiveDecoration>,
        font_metrics: &FontMetrics,
        decoration: &PropagatedDecoration,
    ) {
        let (baseline_offset, thickness) = match decoration.kind {
            DecorationKind::Underline => (
                font_metrics.underline_top_offset,
                font_metrics.underline_thickness,
            ),
            DecorationKind::LineThrough => (
                font_metrics.strikeout_top_offset,
                font_metrics.strikeout_thickness,
            ),
        };

        active.push(ActiveDecoration {
            baseline_offset,
            thickness,
            color: decoration.color,
            kind: decoration.kind,
        });
    }

    pub fn push_decorations(
        &mut self,
        style: &ComputedStyle,
        font_metrics_if_inline: Option<&FontMetrics>,
    ) -> DecorationContext<'_> {
        let scope = if font_metrics_if_inline.is_some() {
            DecorationScope::Inline {
                restore_first_inactive_propagated: self.tracker.first_inactive_propagated,
                restore_active_len: self.tracker.active.len(),
            }
        } else {
            DecorationScope::Other {
                restore_propagated_len: self.tracker.propagated.len(),
            }
        };

        if let Some(font_metrics) = font_metrics_if_inline {
            let to_activate = &self.tracker.propagated[self.tracker.first_inactive_propagated..];

            for decoration in to_activate {
                Self::push_active_decoration(&mut self.tracker.active, font_metrics, decoration);
            }

            self.tracker.first_inactive_propagated = self.tracker.propagated.len();
        }

        let mut push_decoration = |decoration: PropagatedDecoration| {
            if let Some(font_metrics) = font_metrics_if_inline {
                Self::push_active_decoration(&mut self.tracker.active, font_metrics, &decoration);
            } else {
                self.tracker.propagated.push(decoration);
            }
        };

        let decoration = style.text_decoration();
        if decoration.underline {
            push_decoration(PropagatedDecoration {
                color: decoration.underline_color,
                kind: DecorationKind::Underline,
            });
        }

        if decoration.line_through {
            push_decoration(PropagatedDecoration {
                color: decoration.line_through_color,
                kind: DecorationKind::LineThrough,
            });
        }

        DecorationContext {
            tracker: &mut *self.tracker,
            scope,
        }
    }

    pub fn suspend_active(&mut self) -> DecorationContext<'_> {
        let restore_first_active = self.tracker.first_active;
        self.tracker.first_active = self.tracker.active.len();

        DecorationContext {
            tracker: &mut *self.tracker,
            scope: DecorationScope::Suspend {
                restore_first_active,
            },
        }
    }
}

impl Drop for DecorationContext<'_> {
    fn drop(&mut self) {
        match self.scope {
            DecorationScope::Suspend {
                restore_first_active,
            } => {
                self.tracker.first_active = restore_first_active;
            }
            DecorationScope::Inline {
                restore_first_inactive_propagated,
                restore_active_len,
            } => {
                self.tracker.first_inactive_propagated = restore_first_inactive_propagated;
                self.tracker.active.truncate(restore_active_len);
            }
            DecorationScope::Other {
                restore_propagated_len,
            } => {
                self.tracker.propagated.truncate(restore_propagated_len);
            }
        }
    }
}
