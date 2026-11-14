use core::ops::Range;

pub(crate) struct Reader<'a> {
    buf: &'a [u8],
    position: usize,
}

impl<'a> Reader<'a> {
    pub(crate) fn new(buf: &'a [u8]) -> Self {
        Self { buf, position: 0 }
    }

    pub(crate) fn position(&self) -> usize {
        self.position
    }

    pub(crate) fn take(&mut self, len: usize) -> Option<&'a [u8]> {
        self.take_range(len).map(|range| &self.buf[range])
    }

    pub(crate) fn skip(&mut self, len: usize) -> Option<()> {
        self.take_range(len).map(drop)
    }

    pub(crate) fn expect_u8(&mut self, expected: u8) -> Option<()> {
        (self.u8()? == expected).then_some(())
    }

    pub(crate) fn u8(&mut self) -> Option<u8> {
        self.array::<1>().map(u8::from_be_bytes)
    }

    pub(crate) fn u16(&mut self) -> Option<u16> {
        self.array().map(u16::from_be_bytes)
    }

    pub(crate) fn u32(&mut self) -> Option<u32> {
        self.array().map(u32::from_be_bytes)
    }

    pub(crate) fn array<const N: usize>(&mut self) -> Option<[u8; N]> {
        self.take(N)?.try_into().ok()
    }

    pub(crate) fn u8_prefixed(&mut self) -> Option<Range<usize>> {
        let len = self.u8()?;
        self.take_range(len.into())
    }

    pub(crate) fn u16_prefixed(&mut self) -> Option<Range<usize>> {
        let len = self.u16()?;
        self.take_range(len.into())
    }

    fn take_range(&mut self, len: usize) -> Option<Range<usize>> {
        let end = self.position.checked_add(len)?;
        if end > self.buf.len() {
            return None;
        }
        let range = self.position..end;
        self.position = end;
        Some(range)
    }
}
