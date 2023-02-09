use std::fmt::Display;

use crate::interval_checking::SExp;

use super::{Id, Range, TimeRep, TimeSub};
use itertools::Itertools;
use smallvec::{smallvec, SmallVec};

/// Build a new offset vec by adding up constants when possible
fn smart_add(old: &mut SmallVec<[u64; 1]>, new: impl Iterator<Item = u64>) {
    for v in new {
        old[0] += v;
    }
}

#[derive(Clone, PartialEq, Eq, Hash)]
/// Represents expression of the form `G+1+k`
pub struct Time<Offset> {
    /// The event for the time expression
    pub event: Id,
    /// The offsets for the time expression
    offsets: SmallVec<[Offset; 1]>,
}

impl Time<u64> {
    pub fn offset(&self) -> u64 {
        assert!(
            self.offsets.len() == 1,
            "Time::offset called on non-concrete time"
        );
        self.offsets[0]
    }
}

impl Display for Time<u64> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.event)?;
        for offset in &self.offsets {
            if *offset != 0 {
                write!(f, "+{}", offset)?;
            }
        }
        Ok(())
    }
}

impl TimeRep for Time<u64> {
    type SubRep = TimeSub<Self>;

    fn unit(event: Id, state: u64) -> Self {
        Self {
            event,
            offsets: smallvec![state],
        }
    }

    fn increment(mut self, n: u64) -> Self {
        debug_assert!(
            self.offsets.len() == 1,
            "Time::increment called on non-concrete time"
        );
        smart_add(&mut self.offsets, std::iter::once(n));
        self
    }

    fn resolve(&self, bindings: &super::Binding<Self>) -> Self {
        let mut n = bindings.get(&self.event).clone();
        smart_add(&mut n.offsets, self.offsets.iter().cloned());
        n
    }

    fn sub(self, other: Self) -> Self::SubRep {
        TimeSub::build(self, other)
    }

    fn event(&self) -> Id {
        self.event.clone()
    }
}

impl From<Time<u64>> for SExp {
    fn from(t: Time<u64>) -> SExp {
        SExp(format!(
            "(+ {} {})",
            t.event,
            t.offsets.iter().map(|v| v.to_string()).join(" ")
        ))
    }
}

impl Range<Time<u64>> {
    /// Convert this interval into an offset. Only possible when interval uses
    /// exactly one event for both start and end.
    pub fn as_offset(&self) -> Option<(Id, u64, u64)> {
        let Range { start, end, .. } = &self;
        if start.event == end.event {
            Some((start.event.clone(), start.offsets[0], end.offsets[0]))
        } else {
            None
        }
    }
}

impl TimeSub<Time<u64>> {
    /// Attempt to automatically simplify the difference when possible
    pub fn build(l: Time<u64>, r: Time<u64>) -> Self {
        if l.event == r.event {
            let l = l.offsets[0];
            let r = r.offsets[0];
            if l > r {
                return Self::Unit(l - r);
            } else {
                return Self::Unit(r - l);
            }
        }
        Self::Sym { l, r }
    }
}
