use core::{mem, ops::Range};

pub const FRAME_WIDTH: usize = 320;
pub const FRAME_HEIGHT: usize = 172;
pub const FRAME_PIXELS: usize = FRAME_WIDTH * FRAME_HEIGHT;
pub const FRAME_BYTES: usize = FRAME_PIXELS * mem::size_of::<u16>();
pub const DOUBLE_FRAME_BYTES: usize = FRAME_BYTES * 2;

pub const DMA_STAGING_LINES: usize = 32;
pub const DMA_STAGING_BYTES: usize = FRAME_WIDTH * DMA_STAGING_LINES * mem::size_of::<u16>();

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DisplayBufferError {
    MisalignedPsram,
    InsufficientPsram,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DirtyBand {
    pub start_row: usize,
    pub row_count: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BufferRoles {
    displayed: usize,
    render: usize,
}

impl BufferRoles {
    pub const fn new() -> Self {
        Self {
            displayed: 0,
            render: 1,
        }
    }

    pub const fn displayed_index(&self) -> usize {
        self.displayed
    }

    pub const fn render_index(&self) -> usize {
        self.render
    }

    pub fn commit_present(&mut self) -> (usize, usize) {
        let new_displayed = self.render;
        let new_render = self.displayed;
        self.displayed = new_displayed;
        self.render = new_render;
        (self.displayed, self.render)
    }
}

impl Default for BufferRoles {
    fn default() -> Self {
        Self::new()
    }
}

pub struct DirtyRows {
    rows: [bool; FRAME_HEIGHT],
}

impl DirtyRows {
    pub const fn new() -> Self {
        Self {
            rows: [false; FRAME_HEIGHT],
        }
    }

    pub fn clear(&mut self) {
        self.rows.fill(false);
    }

    pub fn mark_all(&mut self) {
        self.rows.fill(true);
    }

    pub fn mark_range(&mut self, y: usize, h: usize) {
        if h == 0 || y >= FRAME_HEIGHT {
            return;
        }
        let end = y.saturating_add(h).min(FRAME_HEIGHT);
        for row in &mut self.rows[y..end] {
            *row = true;
        }
    }

    pub fn retain_differences(&mut self, displayed: &[u16], render: &[u16]) {
        debug_assert_eq!(displayed.len(), FRAME_PIXELS);
        debug_assert_eq!(render.len(), FRAME_PIXELS);

        for (row_idx, dirty) in self.rows.iter_mut().enumerate() {
            if !*dirty {
                continue;
            }
            let span = row_span(row_idx);
            if displayed[span.clone()] == render[span] {
                *dirty = false;
            }
        }
    }

    pub fn any(&self) -> bool {
        self.rows.iter().any(|row| *row)
    }

    pub fn bands(&self) -> DirtyBandIter<'_> {
        DirtyBandIter {
            rows: &self.rows,
            cursor: 0,
        }
    }
}

impl Default for DirtyRows {
    fn default() -> Self {
        Self::new()
    }
}

pub struct DirtyBandIter<'a> {
    rows: &'a [bool; FRAME_HEIGHT],
    cursor: usize,
}

impl Iterator for DirtyBandIter<'_> {
    type Item = DirtyBand;

    fn next(&mut self) -> Option<Self::Item> {
        while self.cursor < FRAME_HEIGHT && !self.rows[self.cursor] {
            self.cursor += 1;
        }
        if self.cursor >= FRAME_HEIGHT {
            return None;
        }

        let start = self.cursor;
        while self.cursor < FRAME_HEIGHT && self.rows[self.cursor] {
            self.cursor += 1;
        }

        Some(DirtyBand {
            start_row: start,
            row_count: self.cursor - start,
        })
    }
}

pub struct DisplayBuffers {
    frame_a: &'static mut [u16],
    frame_b: &'static mut [u16],
    roles: BufferRoles,
}

impl DisplayBuffers {
    pub unsafe fn from_psram_raw_parts(
        ptr: *mut u8,
        available_bytes: usize,
    ) -> Result<Self, DisplayBufferError> {
        let aligned = ptr.align_offset(mem::align_of::<u16>());
        if aligned == usize::MAX || available_bytes < aligned {
            return Err(DisplayBufferError::MisalignedPsram);
        }

        let usable_ptr = ptr.add(aligned);
        let usable_bytes = available_bytes - aligned;
        if usable_bytes < DOUBLE_FRAME_BYTES {
            return Err(DisplayBufferError::InsufficientPsram);
        }

        let words = core::slice::from_raw_parts_mut(
            usable_ptr.cast::<u16>(),
            usable_bytes / mem::size_of::<u16>(),
        );
        let (frame_a, remainder) = words.split_at_mut(FRAME_PIXELS);
        let (frame_b, _) = remainder.split_at_mut(FRAME_PIXELS);
        frame_a.fill(0);
        frame_b.fill(0);

        Ok(Self {
            frame_a,
            frame_b,
            roles: BufferRoles::new(),
        })
    }

    pub fn displayed(&self) -> &[u16] {
        match self.roles.displayed_index() {
            0 => self.frame_a,
            _ => self.frame_b,
        }
    }

    pub fn render(&self) -> &[u16] {
        match self.roles.render_index() {
            0 => self.frame_a,
            _ => self.frame_b,
        }
    }

    pub fn render_mut(&mut self) -> &mut [u16] {
        match self.roles.render_index() {
            0 => self.frame_a,
            _ => self.frame_b,
        }
    }

    pub fn copy_displayed_to_render(&mut self) {
        let displayed_index = self.roles.displayed_index();
        let render_index = self.roles.render_index();
        if displayed_index == render_index {
            return;
        }

        match (displayed_index, render_index) {
            (0, 1) => self.frame_b.copy_from_slice(self.frame_a),
            (1, 0) => self.frame_a.copy_from_slice(self.frame_b),
            _ => unreachable!(),
        }
    }

    pub fn commit_present(&mut self) -> (usize, usize) {
        self.roles.commit_present()
    }
}

pub fn row_span(row: usize) -> Range<usize> {
    let start = row * FRAME_WIDTH;
    start..start + FRAME_WIDTH
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{vec, vec::Vec};

    #[test]
    fn dirty_rows_merge_into_full_width_bands() {
        let mut dirty = DirtyRows::new();
        dirty.mark_range(2, 3);
        dirty.mark_range(8, 2);

        let bands: Vec<_> = dirty.bands().collect();
        assert_eq!(
            bands,
            vec![
                DirtyBand {
                    start_row: 2,
                    row_count: 3,
                },
                DirtyBand {
                    start_row: 8,
                    row_count: 2,
                },
            ]
        );
    }

    #[test]
    fn retain_differences_clears_matching_rows() {
        let mut dirty = DirtyRows::new();
        dirty.mark_range(4, 2);

        let mut displayed = vec![0u16; FRAME_PIXELS];
        let mut render = displayed.clone();
        render[row_span(5)].fill(0xAAAA);

        dirty.retain_differences(&displayed, &render);

        let bands: Vec<_> = dirty.bands().collect();
        assert_eq!(
            bands,
            vec![DirtyBand {
                start_row: 5,
                row_count: 1,
            }]
        );

        displayed[row_span(5)].fill(0xAAAA);
        dirty.mark_range(5, 1);
        dirty.retain_differences(&displayed, &render);
        assert!(!dirty.any());
    }

    #[test]
    fn buffer_roles_rotate_after_present() {
        let mut roles = BufferRoles::new();
        assert_eq!(roles.displayed_index(), 0);
        assert_eq!(roles.render_index(), 1);

        let (displayed, render) = roles.commit_present();
        assert_eq!((displayed, render), (1, 0));

        let (displayed, render) = roles.commit_present();
        assert_eq!((displayed, render), (0, 1));
    }
}
