use core::ops::Range;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Inspection {
    Host(Range<usize>),
    NeedMoreData,
    NoHost,
}
