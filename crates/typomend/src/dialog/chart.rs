// Copyright boylin0 and the Typomend contributors.
// SPDX-License-Identifier: MPL-2.0
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! A line chart drawn with GDI into an owner-drawn static control.
//!
//! The Usage page shows Typomend's own process as a single line in one color.
//! The chart is painted into an off-screen bitmap and copied over in one
//! step, so redrawing it every second does not flicker. Colors other than the
//! line come from the system palette, which keeps the chart readable under
//! high-contrast themes.

use windows::Win32::Foundation::{COLORREF, POINT, RECT};
use windows::Win32::Graphics::Gdi::{
    BitBlt, COLOR_3DSHADOW, COLOR_BTNFACE, COLOR_GRAYTEXT, COLOR_WINDOW, COLOR_WINDOWTEXT,
    CreateCompatibleBitmap, CreateCompatibleDC, CreatePen, CreateSolidBrush, DT_CALCRECT, DT_LEFT,
    DT_NOPREFIX, DT_SINGLELINE, DT_TOP, DeleteDC, DeleteObject, DrawTextW, FillRect, GetSysColor,
    HDC, HFONT, HGDIOBJ, PS_SOLID, Polyline, SRCCOPY, SYS_COLOR_INDEX, SelectObject, SetBkMode,
    SetTextColor, TRANSPARENT,
};
use windows::Win32::UI::Controls::DRAWITEMSTRUCT;

/// Line color: the brand's Mend Blue.
pub const BLUE: COLORREF = rgb(0x25, 0x63, 0xEB);

/// One line of the chart.
#[derive(Debug)]
pub struct Series {
    /// Legend text, such as "Typomend 2.5%".
    pub legend: String,
    /// Color of the line and of its legend entry.
    pub color: COLORREF,
    /// Values oldest first, the newest drawn at the right edge.
    pub values: Vec<f32>,
}

/// What to draw.
#[derive(Debug)]
pub struct Chart<'a> {
    /// Text at the top left, such as "CPU (up to 25%)".
    pub title: String,
    /// The value at the top of the plot.
    pub max: f32,
    /// How many values fit across the plot.
    pub capacity: usize,
    /// The lines to draw, each with its own legend entry.
    pub series: &'a [Series],
}

const fn rgb(r: u8, g: u8, b: u8) -> COLORREF {
    COLORREF(r as u32 | (g as u32) << 8 | (b as u32) << 16)
}

fn system_color(index: SYS_COLOR_INDEX) -> COLORREF {
    // SAFETY: GetSysColor takes a plain index and has no pointer arguments or
    // other preconditions.
    COLORREF(unsafe { GetSysColor(index) })
}

/// Paints `chart` into the control described by `item`, in `font`.
///
/// Does nothing when the control has no area.
pub fn draw(item: &DRAWITEMSTRUCT, font: HFONT, chart: &Chart) {
    let rect = item.rcItem;
    let width = rect.right - rect.left;
    let height = rect.bottom - rect.top;
    if width <= 0 || height <= 0 {
        return;
    }
    // SAFETY: `item.hDC` is the control's DC and stays valid for the duration
    // of WM_DRAWITEM. The memory DC and bitmap created here are deselected
    // and deleted before returning, and `paint` gets a bitmap of `width` by
    // `height`, which covers the area it is given.
    unsafe {
        let dc = CreateCompatibleDC(Some(item.hDC));
        let bitmap = CreateCompatibleBitmap(item.hDC, width, height);
        let old_bitmap = SelectObject(dc, HGDIOBJ(bitmap.0));
        let old_font = SelectObject(dc, HGDIOBJ(font.0));
        paint(
            dc,
            RECT {
                left: 0,
                top: 0,
                right: width,
                bottom: height,
            },
            chart,
        );
        let _ = BitBlt(
            item.hDC,
            rect.left,
            rect.top,
            width,
            height,
            Some(dc),
            0,
            0,
            SRCCOPY,
        );
        SelectObject(dc, old_font);
        SelectObject(dc, old_bitmap);
        let _ = DeleteObject(HGDIOBJ(bitmap.0));
        let _ = DeleteDC(dc);
    }
}

/// Paints the whole chart into `area` of `dc`.
///
/// # Safety
///
/// `dc` must be a valid memory DC with a bitmap covering `area`.
unsafe fn paint(dc: HDC, area: RECT, chart: &Chart) {
    // SAFETY: the caller guarantees `dc` is valid. Every brush and pen created
    // here is deselected before it is deleted, and the helpers called get the
    // same valid `dc`.
    unsafe {
        let background = CreateSolidBrush(system_color(COLOR_WINDOW));
        FillRect(dc, &area, background);
        let _ = DeleteObject(HGDIOBJ(background.0));
        SetBkMode(dc, TRANSPARENT);

        // Header: the title at the left, the legend entries at the right.
        let line_height = text_size(dc, "Ag").1;
        let pad = line_height / 4;
        SetTextColor(dc, system_color(COLOR_WINDOWTEXT));
        text(dc, &chart.title, area.left + pad, area.top + pad);
        let mut right = area.right - pad;
        for series in chart.series.iter().rev() {
            let (w, _) = text_size(dc, &series.legend);
            right -= w;
            SetTextColor(dc, series.color);
            text(dc, &series.legend, right, area.top + pad);
            right -= line_height;
        }

        let plot = RECT {
            left: area.left + pad,
            top: area.top + line_height + pad * 2,
            right: area.right - pad,
            bottom: area.bottom - pad,
        };
        if plot.bottom - plot.top < 4 || plot.right - plot.left < 4 {
            return;
        }

        // Grid lines at the quarters, then the frame.
        let grid = CreatePen(PS_SOLID, 1, system_color(COLOR_BTNFACE));
        let old_pen = SelectObject(dc, HGDIOBJ(grid.0));
        for quarter in 1..4 {
            let y = plot.bottom - (plot.bottom - plot.top) * quarter / 4;
            let _ = Polyline(dc, &[POINT { x: plot.left, y }, POINT { x: plot.right, y }]);
        }
        let frame = CreatePen(PS_SOLID, 1, system_color(COLOR_3DSHADOW));
        SelectObject(dc, HGDIOBJ(frame.0));
        let _ = Polyline(
            dc,
            &[
                POINT {
                    x: plot.left,
                    y: plot.top,
                },
                POINT {
                    x: plot.right - 1,
                    y: plot.top,
                },
                POINT {
                    x: plot.right - 1,
                    y: plot.bottom - 1,
                },
                POINT {
                    x: plot.left,
                    y: plot.bottom - 1,
                },
                POINT {
                    x: plot.left,
                    y: plot.top,
                },
            ],
        );
        SelectObject(dc, old_pen);
        let _ = DeleteObject(HGDIOBJ(grid.0));
        let _ = DeleteObject(HGDIOBJ(frame.0));

        let empty = chart.series.iter().all(|s| s.values.len() < 2);
        if empty {
            SetTextColor(dc, system_color(COLOR_GRAYTEXT));
            text(dc, "...", plot.left + pad, plot.top + pad);
            return;
        }
        for series in chart.series {
            let points = points(&series.values, chart.max, chart.capacity, plot);
            if points.len() < 2 {
                continue;
            }
            let pen = CreatePen(PS_SOLID, 2, series.color);
            let old_pen = SelectObject(dc, HGDIOBJ(pen.0));
            let _ = Polyline(dc, &points);
            SelectObject(dc, old_pen);
            let _ = DeleteObject(HGDIOBJ(pen.0));
        }
    }
}

/// Places `values` in `plot`, the last one at the right edge.
///
/// The plot width is divided into `capacity - 1` equal steps, one per value,
/// so a full series spans the plot. Only the newest `capacity` values are
/// kept. Values above `max` are drawn at the top.
fn points(values: &[f32], max: f32, capacity: usize, plot: RECT) -> Vec<POINT> {
    let step = (plot.right - plot.left - 1) as f32 / capacity.saturating_sub(1).max(1) as f32;
    let span = (plot.bottom - plot.top - 2) as f32;
    let shown = &values[values.len().saturating_sub(capacity)..];
    shown
        .iter()
        .enumerate()
        .map(|(i, &value)| {
            let from_right = (shown.len() - 1 - i) as f32;
            let level = if max > 0.0 {
                (value / max).clamp(0.0, 1.0)
            } else {
                0.0
            };
            POINT {
                x: plot.right - 1 - (from_right * step).round() as i32,
                y: plot.bottom - 2 - (level * span).round() as i32,
            }
        })
        .collect()
}

/// Returns the width and height of `s` in the DC's font.
///
/// # Safety
///
/// `dc` must be a valid DC.
unsafe fn text_size(dc: HDC, s: &str) -> (i32, i32) {
    let mut wide: Vec<u16> = s.encode_utf16().collect();
    let mut rect = RECT::default();
    // SAFETY: `dc` is valid by the caller's contract. DT_CALCRECT only
    // measures, so `wide` is not written, and both buffers outlive the call.
    unsafe {
        DrawTextW(
            dc,
            &mut wide,
            &mut rect,
            DT_CALCRECT | DT_SINGLELINE | DT_NOPREFIX,
        );
    }
    (rect.right - rect.left, rect.bottom - rect.top)
}

/// Draws `s` with its top left corner at `x`, `y`.
///
/// # Safety
///
/// `dc` must be a valid DC.
unsafe fn text(dc: HDC, s: &str, x: i32, y: i32) {
    let mut wide: Vec<u16> = s.encode_utf16().collect();
    let mut rect = RECT {
        left: x,
        top: y,
        right: x + 10_000,
        bottom: y + 10_000,
    };
    // SAFETY: `dc` is valid by the caller's contract, and `wide` and `rect`
    // outlive the call. The DC's bitmap clips anything drawn outside it, so
    // the oversized `rect` is harmless.
    unsafe {
        DrawTextW(
            dc,
            &mut wide,
            &mut rect,
            DT_LEFT | DT_TOP | DT_SINGLELINE | DT_NOPREFIX,
        );
    }
}

/// Returns the smallest round number that is at least `value` and `floor`.
///
/// Round numbers are 1, 2 and 5 times a power of ten, which gives a chart a
/// top value that reads cleanly and does not jump on every new sample.
pub fn nice_max(value: f32, floor: f32) -> f32 {
    let value = value.max(floor);
    let mut magnitude = 10f32.powf(value.log10().floor());
    loop {
        for step in [1.0, 2.0, 5.0] {
            if step * magnitude >= value {
                return step * magnitude;
            }
        }
        magnitude *= 10.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rounds_the_scale_up() {
        assert_eq!(nice_max(0.3, 5.0), 5.0);
        assert_eq!(nice_max(7.0, 5.0), 10.0);
        assert_eq!(nice_max(130.0, 1.0), 200.0);
        assert_eq!(nice_max(480.0, 1.0), 500.0);
        assert_eq!(nice_max(100.0, 1.0), 100.0);
    }

    #[test]
    fn puts_the_newest_value_at_the_right_edge() {
        let plot = RECT {
            left: 0,
            top: 0,
            right: 101,
            bottom: 52,
        };
        let points = points(&[0.0, 50.0, 100.0], 100.0, 11, plot);
        assert_eq!(points.len(), 3);
        assert_eq!(points[2], POINT { x: 100, y: 0 });
        assert_eq!(points[1], POINT { x: 90, y: 25 });
        assert_eq!(points[0], POINT { x: 80, y: 50 });
    }
}
