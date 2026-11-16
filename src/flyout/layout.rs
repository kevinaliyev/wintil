//! Lightweight Declarative Win32 GDI Layout Engine
//!
//! Provides an ergonomic, composable, high-DPI-aware layout tree for Win32 GDI flyouts.
//! Eliminates magic numbers, manual coordinate calculations, and hit-testing coordinate desyncs
//! by computing exact bounding boxes during measure and layout passes.

#![allow(dead_code)]

use windows::Win32::Foundation::{COLORREF, POINT, RECT};
use windows::Win32::Graphics::Gdi::{
    CreatePen, CreateSolidBrush, DeleteObject, DrawTextW, FillRect, IntersectClipRect, RestoreDC,
    RoundRect, SaveDC, SelectObject, SetBkMode, SetTextColor, DT_CALCRECT, DT_CENTER,
    DT_END_ELLIPSIS, DT_LEFT, DT_NOPREFIX, DT_RIGHT, DT_SINGLELINE, DT_VCENTER, DT_WORDBREAK, HDC,
    HFONT, PS_SOLID, TRANSPARENT,
};

use super::{rgb, to_wide, ClickTarget, DpiScale, FlyoutAction, TooltipHit};

/// Physical or logical rectangle for layout positioning.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LayoutRect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

impl LayoutRect {
    #[inline]
    pub const fn new(x: i32, y: i32, width: i32, height: i32) -> Self {
        Self { x, y, width, height }
    }

    #[inline]
    pub const fn from_ltrb(left: i32, top: i32, right: i32, bottom: i32) -> Self {
        Self {
            x: left,
            y: top,
            width: right - left,
            height: bottom - top,
        }
    }

    #[inline]
    pub fn to_win32_rect(&self) -> RECT {
        RECT {
            left: self.x,
            top: self.y,
            right: self.x + self.width,
            bottom: self.y + self.height,
        }
    }

    #[inline]
    pub fn from_win32_rect(rc: &RECT) -> Self {
        Self {
            x: rc.left,
            y: rc.top,
            width: rc.right - rc.left,
            height: rc.bottom - rc.top,
        }
    }

    #[inline]
    pub fn contains(&self, pt: POINT) -> bool {
        pt.x >= self.x
            && pt.x < (self.x + self.width)
            && pt.y >= self.y
            && pt.y < (self.y + self.height)
    }

    #[inline]
    pub fn shrink(&self, insets: EdgeInsets) -> Self {
        let x = self.x + insets.left;
        let y = self.y + insets.top;
        let width = (self.width - (insets.left + insets.right)).max(0);
        let height = (self.height - (insets.top + insets.bottom)).max(0);
        Self { x, y, width, height }
    }

    /// Computes the intersection of two rectangles, returning `None` if they do not overlap.
    #[inline]
    pub fn intersect(&self, other: &Self) -> Option<Self> {
        let left = self.x.max(other.x);
        let top = self.y.max(other.y);
        let right = (self.x + self.width).min(other.x + other.width);
        let bottom = (self.y + self.height).min(other.y + other.height);
        if right > left && bottom > top {
            Some(Self::from_ltrb(left, top, right, bottom))
        } else {
            None
        }
    }
}

/// Stateful scroll controller for scrollable containers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ScrollState {
    pub scroll_offset: i32,
    pub viewport_height: i32,
    pub content_height: i32,
}

impl ScrollState {
    #[inline]
    pub const fn new(viewport_height: i32, content_height: i32) -> Self {
        Self {
            scroll_offset: 0,
            viewport_height,
            content_height,
        }
    }

    #[inline]
    pub fn max_scroll(&self) -> i32 {
        (self.content_height - self.viewport_height).max(0)
    }

    #[inline]
    pub fn is_scrollable(&self) -> bool {
        self.content_height > self.viewport_height && self.viewport_height > 0
    }

    #[inline]
    pub fn clamp(&mut self) {
        self.scroll_offset = self.scroll_offset.clamp(0, self.max_scroll());
    }

    #[inline]
    pub fn scroll_by(&mut self, delta_px: i32) {
        self.scroll_offset = (self.scroll_offset + delta_px).clamp(0, self.max_scroll());
    }

    /// Applies a standard Win32 `WM_MOUSEWHEEL` delta (multiples of 120, positive = wheel up).
    #[inline]
    pub fn apply_wheel_delta(&mut self, wheel_delta: i16, step_px: i32) {
        if wheel_delta == 0 || !self.is_scrollable() {
            return;
        }
        let steps = -(wheel_delta as f32 / 120.0);
        let delta_px = (steps * step_px as f32).round() as i32;
        self.scroll_by(delta_px);
    }

    /// Computes the proportional scrollbar thumb rectangle inside `track_bounds`.
    pub fn thumb_rect(&self, track_bounds: LayoutRect, min_thumb_h: i32) -> Option<LayoutRect> {
        if !self.is_scrollable() || track_bounds.height <= 0 {
            return None;
        }
        let ratio = (self.viewport_height as f32 / self.content_height as f32).clamp(0.05, 1.0);
        let thumb_h = ((track_bounds.height as f32 * ratio).round() as i32)
            .max(min_thumb_h.min(track_bounds.height))
            .min(track_bounds.height);
        let max_s = self.max_scroll().max(1);
        let clamped_off = self.scroll_offset.clamp(0, max_s);
        let avail_track = (track_bounds.height - thumb_h).max(0);
        let thumb_y = track_bounds.y + ((avail_track as f32 * (clamped_off as f32 / max_s as f32)).round() as i32);
        Some(LayoutRect::new(track_bounds.x, thumb_y, track_bounds.width, thumb_h))
    }
}

/// 2D Size dimension.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Size {
    pub width: i32,
    pub height: i32,
}

impl Size {
    #[inline]
    pub const fn new(width: i32, height: i32) -> Self {
        Self { width, height }
    }
}

/// Padding or margin insets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct EdgeInsets {
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
    pub left: i32,
}

impl EdgeInsets {
    pub const ZERO: Self = Self { top: 0, right: 0, bottom: 0, left: 0 };

    #[inline]
    pub const fn all(val: i32) -> Self {
        Self { top: val, right: val, bottom: val, left: val }
    }

    #[inline]
    pub const fn symmetric(vertical: i32, horizontal: i32) -> Self {
        Self {
            top: vertical,
            right: horizontal,
            bottom: vertical,
            left: horizontal,
        }
    }

    #[inline]
    pub const fn trbl(top: i32, right: i32, bottom: i32, left: i32) -> Self {
        Self { top, right, bottom, left }
    }

    #[inline]
    pub const fn horizontal(&self) -> i32 {
        self.left + self.right
    }

    #[inline]
    pub const fn vertical(&self) -> i32 {
        self.top + self.bottom
    }

    #[inline]
    pub fn scale(&self, scale: &DpiScale) -> Self {
        Self {
            top: scale.scale(self.top),
            right: scale.scale(self.right),
            bottom: scale.scale(self.bottom),
            left: scale.scale(self.left),
        }
    }
}

/// Dimension sizing mode.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Length {
    Fixed(i32),
    Auto,
    Flex(f32),
}

/// Primary axis alignment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MainAlign {
    #[default]
    Start,
    Center,
    End,
    SpaceBetween,
}

/// Cross axis alignment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CrossAlign {
    #[default]
    Start,
    Center,
    End,
    Stretch,
}

/// Font style variant used across flyout components.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FontVariant {
    Title,
    Semibold,
    Normal,
    Small,
}

/// Text alignment within bounds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TextAlign {
    #[default]
    Left,
    Center,
    Right,
}

/// Available GDI fonts passed down during measure and render passes.
#[derive(Clone, Copy)]
pub struct LayoutFonts {
    pub title: HFONT,
    pub semibold: HFONT,
    pub normal: HFONT,
    pub small: HFONT,
}

impl LayoutFonts {
    pub fn get_hfont(&self, variant: FontVariant) -> HFONT {
        match variant {
            FontVariant::Title => self.title,
            FontVariant::Semibold => self.semibold,
            FontVariant::Normal => self.normal,
            FontVariant::Small => self.small,
        }
    }

    /// Measures text bounding box using GDI DT_CALCRECT or deterministic fallback in test mode.
    pub fn measure_text(
        &self,
        hdc: HDC,
        text: &str,
        variant: FontVariant,
        scale: &DpiScale,
        max_width: Option<i32>,
    ) -> Size {
        if text.is_empty() {
            return Size::new(0, 0);
        }

        // Test fallback when HDC is null
        if hdc.0.is_null() {
            let char_w = match variant {
                FontVariant::Title => 9,
                FontVariant::Semibold => 8,
                FontVariant::Normal => 7,
                FontVariant::Small => 6,
            };
            let line_h = match variant {
                FontVariant::Title => 20,
                FontVariant::Semibold => 18,
                FontVariant::Normal => 16,
                FontVariant::Small => 14,
            };
            let w = scale.scale(text.len() as i32 * char_w);
            let h = scale.scale(line_h);
            return Size::new(w, h);
        }

        unsafe {
            let hfont = self.get_hfont(variant);
            let old_font = SelectObject(hdc, hfont);
            let mut wide = to_wide(text);
            let mut rc = RECT {
                left: 0,
                top: 0,
                right: max_width.unwrap_or(2000),
                bottom: 0,
            };

            let mut flags = DT_CALCRECT | DT_NOPREFIX;
            if max_width.is_some() {
                flags |= DT_WORDBREAK;
            } else {
                flags |= DT_SINGLELINE;
            }

            DrawTextW(hdc, &mut wide, &mut rc, flags);
            SelectObject(hdc, old_font);

            Size::new(rc.right - rc.left, rc.bottom - rc.top)
        }
    }
}

/// Declarative Layout Node representation.
pub enum LayoutNode {
    Container {
        padding: EdgeInsets,
        margin: EdgeInsets,
        bg_color: Option<COLORREF>,
        border_color: Option<COLORREF>,
        border_radius: i32,
        child: Option<Box<LayoutNode>>,
    },
    Card {
        padding: EdgeInsets,
        margin: EdgeInsets,
        child: Box<LayoutNode>,
    },
    VStack {
        padding: EdgeInsets,
        gap: i32,
        main_align: MainAlign,
        cross_align: CrossAlign,
        children: Vec<LayoutNode>,
    },
    HStack {
        padding: EdgeInsets,
        gap: i32,
        main_align: MainAlign,
        cross_align: CrossAlign,
        children: Vec<(LayoutNode, Length)>,
    },
    Label {
        text: String,
        font: FontVariant,
        color: COLORREF,
        align: TextAlign,
        single_line: bool,
    },
    Button {
        text: String,
        width: Length,
        height: i32,
        font: FontVariant,
        is_active: bool,
        is_danger: bool,
        action: Option<FlyoutAction>,
        tooltip: Option<String>,
    },
    StatusPill {
        dot_color: COLORREF,
        text: String,
        font: FontVariant,
    },
    Divider {
        color: COLORREF,
        thickness: i32,
    },
    Spacer {
        flex: f32,
    },
    ScrollView {
        viewport_height: i32,
        scroll_offset: i32,
        padding: EdgeInsets,
        show_scrollbar: bool,
        child: Box<LayoutNode>,
    },
}

/// Result of layout pass: a positioned node with final physical coordinates.
pub enum PlacedNode {
    Container {
        bounds: LayoutRect,
        bg_color: Option<COLORREF>,
        border_color: Option<COLORREF>,
        border_radius: i32,
        child: Option<Box<PlacedNode>>,
    },
    Card {
        bounds: LayoutRect,
        child: Box<PlacedNode>,
    },
    VStack {
        bounds: LayoutRect,
        children: Vec<PlacedNode>,
    },
    HStack {
        bounds: LayoutRect,
        children: Vec<PlacedNode>,
    },
    Label {
        bounds: LayoutRect,
        text: String,
        font: FontVariant,
        color: COLORREF,
        align: TextAlign,
    },
    Button {
        bounds: LayoutRect,
        text: String,
        font: FontVariant,
        is_active: bool,
        is_danger: bool,
        action: Option<FlyoutAction>,
        tooltip: Option<String>,
    },
    StatusPill {
        bounds: LayoutRect,
        dot_color: COLORREF,
        text: String,
        font: FontVariant,
    },
    Divider {
        bounds: LayoutRect,
        color: COLORREF,
    },
    Spacer {
        bounds: LayoutRect,
    },
    ScrollView {
        bounds: LayoutRect,
        content_height: i32,
        scroll_offset: i32,
        show_scrollbar: bool,
        child: Box<PlacedNode>,
    },
}

impl LayoutNode {
    /// Measure pass: computes minimum/preferred bounding size.
    pub fn measure(
        &self,
        constraint_w: i32,
        constraint_h: i32,
        scale: &DpiScale,
        fonts: &LayoutFonts,
        hdc: HDC,
    ) -> Size {
        match self {
            LayoutNode::Container { padding, margin, child, .. } => {
                let p = padding.scale(scale);
                let m = margin.scale(scale);
                let inner_cw = (constraint_w - p.horizontal() - m.horizontal()).max(0);
                let inner_ch = (constraint_h - p.vertical() - m.vertical()).max(0);

                let inner_size = if let Some(ch) = child {
                    ch.measure(inner_cw, inner_ch, scale, fonts, hdc)
                } else {
                    Size::new(0, 0)
                };

                Size::new(
                    inner_size.width + p.horizontal() + m.horizontal(),
                    inner_size.height + p.vertical() + m.vertical(),
                )
            }

            LayoutNode::Card { padding, margin, child } => {
                let p = padding.scale(scale);
                let m = margin.scale(scale);
                let inner_cw = (constraint_w - p.horizontal() - m.horizontal()).max(0);
                let inner_ch = (constraint_h - p.vertical() - m.vertical()).max(0);

                let inner_size = child.measure(inner_cw, inner_ch, scale, fonts, hdc);
                Size::new(
                    inner_size.width + p.horizontal() + m.horizontal(),
                    inner_size.height + p.vertical() + m.vertical(),
                )
            }

            LayoutNode::VStack { padding, gap, children, .. } => {
                let p = padding.scale(scale);
                let scaled_gap = scale.scale(*gap);
                let inner_cw = (constraint_w - p.horizontal()).max(0);

                let mut total_h = 0;
                let mut max_w = 0;

                for (idx, ch) in children.iter().enumerate() {
                    let s = ch.measure(inner_cw, constraint_h, scale, fonts, hdc);
                    total_h += s.height;
                    max_w = max_w.max(s.width);
                    if idx > 0 {
                        total_h += scaled_gap;
                    }
                }

                Size::new(max_w + p.horizontal(), total_h + p.vertical())
            }

            LayoutNode::HStack { padding, gap, children, .. } => {
                let p = padding.scale(scale);
                let scaled_gap = scale.scale(*gap);
                let inner_ch = (constraint_h - p.vertical()).max(0);

                let mut total_w = 0;
                let mut max_h = 0;

                for (idx, (ch, len)) in children.iter().enumerate() {
                    let w_override = match len {
                        Length::Fixed(w) => Some(scale.scale(*w)),
                        _ => None,
                    };
                    let s = ch.measure(w_override.unwrap_or(constraint_w), inner_ch, scale, fonts, hdc);
                    let actual_w = w_override.unwrap_or(s.width);
                    total_w += actual_w;
                    max_h = max_h.max(s.height);
                    if idx > 0 {
                        total_w += scaled_gap;
                    }
                }

                Size::new(total_w + p.horizontal(), max_h + p.vertical())
            }

            LayoutNode::Label { text, font, .. } => {
                fonts.measure_text(hdc, text, *font, scale, Some(constraint_w))
            }

            LayoutNode::Button { text, width, height, font, .. } => {
                let text_size = fonts.measure_text(hdc, text, *font, scale, None);
                let pad_x = scale.scale(16);
                let req_w = match width {
                    Length::Fixed(w) => scale.scale(*w),
                    Length::Auto => text_size.width + pad_x,
                    Length::Flex(_) => constraint_w,
                };
                let req_h = scale.scale(*height);
                Size::new(req_w, req_h)
            }

            LayoutNode::StatusPill { text, font, .. } => {
                let text_size = fonts.measure_text(hdc, text, *font, scale, None);
                let dot_w = scale.scale(14);
                Size::new(text_size.width + dot_w, text_size.height)
            }

            LayoutNode::Divider { thickness, .. } => {
                Size::new(constraint_w, scale.scale(*thickness))
            }

            LayoutNode::Spacer { .. } => Size::new(0, 0),

            LayoutNode::ScrollView { viewport_height, padding, show_scrollbar, child, .. } => {
                let p = padding.scale(scale);
                let sb_w = if *show_scrollbar { scale.scale(8) } else { 0 };
                let inner_cw = (constraint_w - p.horizontal() - sb_w).max(0);
                let child_size = child.measure(inner_cw, i32::MAX / 4, scale, fonts, hdc);
                let vp_h = scale.scale(*viewport_height);
                Size::new(
                    child_size.width + p.horizontal() + sb_w,
                    vp_h,
                )
            }
        }
    }

    /// Layout pass: assigns exact screen coordinates based on parent constraints.
    pub fn layout(
        &self,
        bounds: LayoutRect,
        scale: &DpiScale,
        fonts: &LayoutFonts,
        hdc: HDC,
    ) -> PlacedNode {
        match self {
            LayoutNode::Container { padding, margin, bg_color, border_color, border_radius, child } => {
                let content_rc = bounds.shrink(margin.scale(scale)).shrink(padding.scale(scale));
                let placed_child = child.as_ref().map(|c| Box::new(c.layout(content_rc, scale, fonts, hdc)));
                PlacedNode::Container {
                    bounds,
                    bg_color: *bg_color,
                    border_color: *border_color,
                    border_radius: scale.scale(*border_radius),
                    child: placed_child,
                }
            }

            LayoutNode::Card { padding, margin, child } => {
                let content_rc = bounds.shrink(margin.scale(scale)).shrink(padding.scale(scale));
                let placed_child = Box::new(child.layout(content_rc, scale, fonts, hdc));
                PlacedNode::Card {
                    bounds,
                    child: placed_child,
                }
            }

            LayoutNode::VStack { padding, gap, cross_align, children, .. } => {
                let p = padding.scale(scale);
                let scaled_gap = scale.scale(*gap);
                let inner = bounds.shrink(p);

                let mut placed_children = Vec::with_capacity(children.len());
                let mut curr_y = inner.y;

                for ch in children {
                    let s = ch.measure(inner.width, inner.height, scale, fonts, hdc);
                    let (child_x, child_w) = match cross_align {
                        CrossAlign::Start => (inner.x, s.width),
                        CrossAlign::Center => (inner.x + (inner.width - s.width) / 2, s.width),
                        CrossAlign::End => (inner.x + inner.width - s.width, s.width),
                        CrossAlign::Stretch => (inner.x, inner.width),
                    };

                    let child_rc = LayoutRect::new(child_x, curr_y, child_w, s.height);
                    placed_children.push(ch.layout(child_rc, scale, fonts, hdc));
                    curr_y += s.height + scaled_gap;
                }

                PlacedNode::VStack {
                    bounds,
                    children: placed_children,
                }
            }

            LayoutNode::HStack { padding, gap, cross_align, children, .. } => {
                let p = padding.scale(scale);
                let scaled_gap = scale.scale(*gap);
                let inner = bounds.shrink(p);

                // Calculate flex distribution
                let mut fixed_w = 0;
                let mut total_flex = 0.0f32;
                let num_gaps = if children.is_empty() { 0 } else { (children.len() - 1) as i32 };
                let gaps_total = num_gaps * scaled_gap;

                for (ch, len) in children {
                    match len {
                        Length::Fixed(w) => fixed_w += scale.scale(*w),
                        Length::Auto => {
                            let s = ch.measure(inner.width, inner.height, scale, fonts, hdc);
                            fixed_w += s.width;
                        }
                        Length::Flex(f) => total_flex += f,
                    }
                }

                let remaining_w = (inner.width - fixed_w - gaps_total).max(0);

                let mut placed_children = Vec::with_capacity(children.len());
                let mut curr_x = inner.x;

                for (ch, len) in children {
                    let w = match len {
                        Length::Fixed(fw) => scale.scale(*fw),
                        Length::Auto => ch.measure(inner.width, inner.height, scale, fonts, hdc).width,
                        Length::Flex(f) => {
                            if total_flex > 0.0 {
                                ((remaining_w as f32 * f) / total_flex).round() as i32
                            } else {
                                0
                            }
                        }
                    };

                    let s = ch.measure(w, inner.height, scale, fonts, hdc);
                    let (child_y, child_h) = match cross_align {
                        CrossAlign::Start => (inner.y, s.height),
                        CrossAlign::Center => (inner.y + (inner.height - s.height) / 2, s.height),
                        CrossAlign::End => (inner.y + inner.height - s.height, s.height),
                        CrossAlign::Stretch => (inner.y, inner.height),
                    };

                    let child_rc = LayoutRect::new(curr_x, child_y, w, child_h);
                    placed_children.push(ch.layout(child_rc, scale, fonts, hdc));
                    curr_x += w + scaled_gap;
                }

                PlacedNode::HStack {
                    bounds,
                    children: placed_children,
                }
            }

            LayoutNode::Label { text, font, color, align, .. } => PlacedNode::Label {
                bounds,
                text: text.clone(),
                font: *font,
                color: *color,
                align: *align,
            },

            LayoutNode::Button { text, font, is_active, is_danger, action, tooltip, .. } => {
                PlacedNode::Button {
                    bounds,
                    text: text.clone(),
                    font: *font,
                    is_active: *is_active,
                    is_danger: *is_danger,
                    action: action.clone(),
                    tooltip: tooltip.clone(),
                }
            }

            LayoutNode::StatusPill { dot_color, text, font } => PlacedNode::StatusPill {
                bounds,
                dot_color: *dot_color,
                text: text.clone(),
                font: *font,
            },

            LayoutNode::Divider { color, .. } => PlacedNode::Divider {
                bounds,
                color: *color,
            },

            LayoutNode::Spacer { .. } => PlacedNode::Spacer { bounds },

            LayoutNode::ScrollView { scroll_offset, padding, show_scrollbar, child, .. } => {
                let p = padding.scale(scale);
                let inner = bounds.shrink(p);
                let sb_reserve = if *show_scrollbar { scale.scale(8) } else { 0 };
                let content_w = (inner.width - sb_reserve).max(0);
                let measured_child = child.measure(content_w, i32::MAX / 4, scale, fonts, hdc);
                let content_h = measured_child.height.max(inner.height);
                let max_scroll = (measured_child.height - inner.height).max(0);
                let clamped_offset = (*scroll_offset).clamp(0, max_scroll);

                let child_bounds = LayoutRect::new(
                    inner.x,
                    inner.y - clamped_offset,
                    content_w,
                    content_h,
                );
                let placed_child = Box::new(child.layout(child_bounds, scale, fonts, hdc));

                PlacedNode::ScrollView {
                    bounds,
                    content_height: measured_child.height,
                    scroll_offset: clamped_offset,
                    show_scrollbar: *show_scrollbar,
                    child: placed_child,
                }
            }
        }
    }
}

impl PlacedNode {
    /// Render pass: draws all components and registers hit targets and tooltips.
    pub(crate) unsafe fn render(
        &self,
        hdc: HDC,
        scale: &DpiScale,
        fonts: &LayoutFonts,
        hovered_idx: Option<usize>,
        targets: &mut Vec<ClickTarget>,
        tooltips: &mut Vec<TooltipHit>,
    ) {
        match self {
            PlacedNode::Container { bounds, bg_color, border_color, border_radius, child } => {
                if bg_color.is_some() || border_color.is_some() {
                    let win_rc = bounds.to_win32_rect();
                    let pen = CreatePen(PS_SOLID, 1, border_color.unwrap_or(rgb(50, 50, 58)));
                    let brush = CreateSolidBrush(bg_color.unwrap_or(rgb(32, 32, 37)));
                    let old_pen = SelectObject(hdc, pen);
                    let old_brush = SelectObject(hdc, brush);

                    let diam = border_radius.max(&2) * 2;
                    let _ = RoundRect(hdc, win_rc.left, win_rc.top, win_rc.right, win_rc.bottom, diam, diam);

                    SelectObject(hdc, old_pen);
                    SelectObject(hdc, old_brush);
                    let _ = DeleteObject(pen);
                    let _ = DeleteObject(brush);
                }

                if let Some(ch) = child {
                    ch.render(hdc, scale, fonts, hovered_idx, targets, tooltips);
                }
            }

            PlacedNode::Card { bounds, child } => {
                let win_rc = bounds.to_win32_rect();
                super::draw_card_frame(hdc, win_rc, scale);
                child.render(hdc, scale, fonts, hovered_idx, targets, tooltips);
            }

            PlacedNode::VStack { children, .. } | PlacedNode::HStack { children, .. } => {
                for ch in children {
                    ch.render(hdc, scale, fonts, hovered_idx, targets, tooltips);
                }
            }

            PlacedNode::Label { bounds, text, font, color, align } => {
                let hfont = fonts.get_hfont(*font);
                SelectObject(hdc, hfont);
                SetTextColor(hdc, *color);
                SetBkMode(hdc, TRANSPARENT);

                let mut wide = to_wide(text);
                let mut win_rc = bounds.to_win32_rect();

                let align_flag = match align {
                    TextAlign::Left => DT_LEFT,
                    TextAlign::Center => DT_CENTER,
                    TextAlign::Right => DT_RIGHT,
                };

                DrawTextW(
                    hdc,
                    &mut wide,
                    &mut win_rc,
                    align_flag | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS | DT_NOPREFIX,
                );
            }

            PlacedNode::Button { bounds, text, font, is_active, is_danger, action, tooltip } => {
                let win_rc = bounds.to_win32_rect();
                let is_hovered = hovered_idx == Some(targets.len());

                if !hdc.0.is_null() {
                    super::draw_button(
                        hdc,
                        win_rc,
                        text,
                        fonts.get_hfont(*font),
                        scale,
                        is_hovered,
                        *is_active,
                        *is_danger,
                    );
                }

                if let Some(act) = action {
                    targets.push(ClickTarget {
                        rect: win_rc,
                        action: act.clone(),
                    });
                }

                if let Some(tt) = tooltip {
                    tooltips.push(TooltipHit {
                        rect: win_rc,
                        text: tt.clone(),
                    });
                }
            }

            PlacedNode::StatusPill { bounds, dot_color, text, font } => {
                let hfont = fonts.get_hfont(*font);
                SelectObject(hdc, hfont);
                SetTextColor(hdc, *dot_color);
                SetBkMode(hdc, TRANSPARENT);

                let mut wide = to_wide(text);
                let mut win_rc = bounds.to_win32_rect();
                DrawTextW(
                    hdc,
                    &mut wide,
                    &mut win_rc,
                    DT_RIGHT | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX,
                );
            }

            PlacedNode::Divider { bounds, color } => {
                let win_rc = bounds.to_win32_rect();
                let brush = CreateSolidBrush(*color);
                FillRect(hdc, &win_rc, brush);
                let _ = DeleteObject(brush);
            }

            PlacedNode::Spacer { .. } => {}

            PlacedNode::ScrollView { bounds, content_height, scroll_offset, show_scrollbar, child } => {
                let win_rc = bounds.to_win32_rect();
                let saved_dc = if !hdc.0.is_null() {
                    let s = SaveDC(hdc);
                    let _ = IntersectClipRect(hdc, win_rc.left, win_rc.top, win_rc.right, win_rc.bottom);
                    s
                } else {
                    0
                };

                let mut inner_targets = Vec::new();
                let mut inner_tooltips = Vec::new();
                child.render(hdc, scale, fonts, None, &mut inner_targets, &mut inner_tooltips);

                // Clip registered hit targets and tooltips strictly to the visible ScrollView viewport
                for t in inner_targets {
                    let t_lr = LayoutRect::from_win32_rect(&t.rect);
                    if let Some(clipped) = bounds.intersect(&t_lr) {
                        targets.push(ClickTarget {
                            rect: clipped.to_win32_rect(),
                            action: t.action,
                        });
                    }
                }

                for tt in inner_tooltips {
                    let tt_lr = LayoutRect::from_win32_rect(&tt.rect);
                    if let Some(clipped) = bounds.intersect(&tt_lr) {
                        tooltips.push(TooltipHit {
                            rect: clipped.to_win32_rect(),
                            text: tt.text,
                        });
                    }
                }

                if *show_scrollbar && *content_height > bounds.height && !hdc.0.is_null() {
                    let sb_w = scale.scale(4).max(2);
                    let track_bounds = LayoutRect::new(
                        bounds.x + bounds.width - sb_w - scale.scale(2),
                        bounds.y + scale.scale(2),
                        sb_w,
                        (bounds.height - scale.scale(4)).max(0),
                    );
                    let state = ScrollState {
                        scroll_offset: *scroll_offset,
                        viewport_height: bounds.height,
                        content_height: *content_height,
                    };
                    if let Some(thumb) = state.thumb_rect(track_bounds, scale.scale(20)) {
                        let track_rc = track_bounds.to_win32_rect();
                        let track_brush = CreateSolidBrush(rgb(36, 36, 42));
                        FillRect(hdc, &track_rc, track_brush);
                        let _ = DeleteObject(track_brush);

                        let thumb_rc = thumb.to_win32_rect();
                        let thumb_brush = CreateSolidBrush(rgb(82, 82, 94));
                        FillRect(hdc, &thumb_rc, thumb_brush);
                        let _ = DeleteObject(thumb_brush);
                    }
                }

                if !hdc.0.is_null() && saved_dc != 0 {
                    let _ = RestoreDC(hdc, saved_dc);
                }
            }
        }
    }
}

// Fluent Builder Constructors
impl LayoutNode {
    pub fn vstack() -> VStackBuilder {
        VStackBuilder::default()
    }

    pub fn hstack() -> HStackBuilder {
        HStackBuilder::default()
    }

    pub fn scroll_view(viewport_height: i32, child: LayoutNode) -> ScrollViewBuilder {
        ScrollViewBuilder {
            viewport_height,
            scroll_offset: 0,
            padding: EdgeInsets::ZERO,
            show_scrollbar: true,
            child: Box::new(child),
        }
    }

    pub fn label(text: impl Into<String>, font: FontVariant, color: COLORREF) -> Self {
        Self::Label {
            text: text.into(),
            font,
            color,
            align: TextAlign::Left,
            single_line: true,
        }
    }

    pub fn button(text: impl Into<String>, width: Length, height: i32, action: FlyoutAction) -> Self {
        Self::Button {
            text: text.into(),
            width,
            height,
            font: FontVariant::Normal,
            is_active: false,
            is_danger: false,
            action: Some(action),
            tooltip: None,
        }
    }
}

#[derive(Default)]
pub struct VStackBuilder {
    padding: EdgeInsets,
    gap: i32,
    cross_align: CrossAlign,
    children: Vec<LayoutNode>,
}

impl VStackBuilder {
    pub fn padding(mut self, insets: EdgeInsets) -> Self {
        self.padding = insets;
        self
    }

    pub fn gap(mut self, g: i32) -> Self {
        self.gap = g;
        self
    }

    pub fn cross_align(mut self, align: CrossAlign) -> Self {
        self.cross_align = align;
        self
    }

    pub fn child(mut self, node: LayoutNode) -> Self {
        self.children.push(node);
        self
    }

    pub fn build(self) -> LayoutNode {
        LayoutNode::VStack {
            padding: self.padding,
            gap: self.gap,
            main_align: MainAlign::Start,
            cross_align: self.cross_align,
            children: self.children,
        }
    }
}

#[derive(Default)]
pub struct HStackBuilder {
    padding: EdgeInsets,
    gap: i32,
    cross_align: CrossAlign,
    children: Vec<(LayoutNode, Length)>,
}

impl HStackBuilder {
    pub fn padding(mut self, insets: EdgeInsets) -> Self {
        self.padding = insets;
        self
    }

    pub fn gap(mut self, g: i32) -> Self {
        self.gap = g;
        self
    }

    pub fn cross_align(mut self, align: CrossAlign) -> Self {
        self.cross_align = align;
        self
    }

    pub fn child_fixed(mut self, node: LayoutNode, width: i32) -> Self {
        self.children.push((node, Length::Fixed(width)));
        self
    }

    pub fn child_auto(mut self, node: LayoutNode) -> Self {
        self.children.push((node, Length::Auto));
        self
    }

    pub fn child_flex(mut self, node: LayoutNode, flex: f32) -> Self {
        self.children.push((node, Length::Flex(flex)));
        self
    }

    pub fn build(self) -> LayoutNode {
        LayoutNode::HStack {
            padding: self.padding,
            gap: self.gap,
            main_align: MainAlign::Start,
            cross_align: self.cross_align,
            children: self.children,
        }
    }
}

pub struct ScrollViewBuilder {
    viewport_height: i32,
    scroll_offset: i32,
    padding: EdgeInsets,
    show_scrollbar: bool,
    child: Box<LayoutNode>,
}

impl ScrollViewBuilder {
    pub fn scroll_offset(mut self, offset: i32) -> Self {
        self.scroll_offset = offset;
        self
    }

    pub fn padding(mut self, insets: EdgeInsets) -> Self {
        self.padding = insets;
        self
    }

    pub fn show_scrollbar(mut self, show: bool) -> Self {
        self.show_scrollbar = show;
        self
    }

    pub fn build(self) -> LayoutNode {
        LayoutNode::ScrollView {
            viewport_height: self.viewport_height,
            scroll_offset: self.scroll_offset,
            padding: self.padding,
            show_scrollbar: self.show_scrollbar,
            child: self.child,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_layout_rect_conversions() {
        let lr = LayoutRect::new(10, 20, 100, 50);
        let win_rc = lr.to_win32_rect();
        assert_eq!(win_rc.left, 10);
        assert_eq!(win_rc.top, 20);
        assert_eq!(win_rc.right, 110);
        assert_eq!(win_rc.bottom, 70);

        let roundtrip = LayoutRect::from_win32_rect(&win_rc);
        assert_eq!(roundtrip, lr);
        assert!(lr.contains(POINT { x: 50, y: 40 }));
        assert!(!lr.contains(POINT { x: 5, y: 40 }));
        assert!(!lr.contains(POINT { x: 120, y: 40 }));
    }

    #[test]
    fn test_edge_insets_shrink() {
        let lr = LayoutRect::new(0, 0, 200, 100);
        let insets = EdgeInsets::trbl(10, 20, 15, 25);
        let shrunk = lr.shrink(insets);
        assert_eq!(shrunk.x, 25);
        assert_eq!(shrunk.y, 10);
        assert_eq!(shrunk.width, 155);
        assert_eq!(shrunk.height, 75);
    }

    #[test]
    fn test_vstack_measure_and_layout() {
        let scale = DpiScale { dpi: 96 };
        let fonts = LayoutFonts {
            title: HFONT(0 as _),
            semibold: HFONT(0 as _),
            normal: HFONT(0 as _),
            small: HFONT(0 as _),
        };
        let hdc = HDC(0 as _);

        let vstack = LayoutNode::vstack()
            .gap(6)
            .padding(EdgeInsets::all(8))
            .child(LayoutNode::button("Btn 1", Length::Auto, 28, FlyoutAction::TestHud))
            .child(LayoutNode::button("Btn 2", Length::Auto, 28, FlyoutAction::TestHud))
            .build();

        let measured = vstack.measure(300, 500, &scale, &fonts, hdc);
        // Padding: 8 top + 8 bottom = 16.
        // 2 Buttons of height 28 each + 1 gap of 6 = 28 + 28 + 6 = 62.
        // Total height = 62 + 16 = 78.
        assert_eq!(measured.height, 78);

        let bounds = LayoutRect::new(0, 0, 300, 78);
        let placed = vstack.layout(bounds, &scale, &fonts, hdc);

        if let PlacedNode::VStack { children, .. } = placed {
            assert_eq!(children.len(), 2);
            if let PlacedNode::Button { bounds: b1, .. } = &children[0] {
                assert_eq!(b1.y, 8); // Top padding
                assert_eq!(b1.height, 28);
            } else {
                panic!("Expected button");
            }
            if let PlacedNode::Button { bounds: b2, .. } = &children[1] {
                assert_eq!(b2.y, 8 + 28 + 6); // 42
                assert_eq!(b2.height, 28);
            } else {
                panic!("Expected button");
            }
        } else {
            panic!("Expected PlacedNode::VStack");
        }
    }

    #[test]
    fn test_hstack_flex_distribution() {
        let scale = DpiScale { dpi: 96 };
        let fonts = LayoutFonts {
            title: HFONT(0 as _),
            semibold: HFONT(0 as _),
            normal: HFONT(0 as _),
            small: HFONT(0 as _),
        };
        let hdc = HDC(0 as _);

        let hstack = LayoutNode::hstack()
            .gap(10)
            .padding(EdgeInsets::all(10))
            .child_fixed(LayoutNode::button("Fixed", Length::Fixed(100), 30, FlyoutAction::TestHud), 100)
            .child_flex(LayoutNode::button("Flex 1", Length::Flex(1.0), 30, FlyoutAction::TestHud), 1.0)
            .child_flex(LayoutNode::button("Flex 2", Length::Flex(2.0), 30, FlyoutAction::TestHud), 2.0)
            .build();

        let bounds = LayoutRect::new(0, 0, 400, 50);
        let placed = hstack.layout(bounds, &scale, &fonts, hdc);

        if let PlacedNode::HStack { children, .. } = placed {
            assert_eq!(children.len(), 3);
            if let PlacedNode::Button { bounds: b0, .. } = &children[0] {
                assert_eq!(b0.x, 10);
                assert_eq!(b0.width, 100);
            }
            if let PlacedNode::Button { bounds: b1, .. } = &children[1] {
                assert_eq!(b1.x, 120); // 10 + 100 + 10
                assert_eq!(b1.width, 87);
            }
            if let PlacedNode::Button { bounds: b2, .. } = &children[2] {
                assert_eq!(b2.x, 120 + 87 + 10); // 217
                assert_eq!(b2.width, 173);
            }
        } else {
            panic!("Expected PlacedNode::HStack");
        }
    }

    #[test]
    fn test_scroll_state_wheel_and_thumb() {
        let mut st = ScrollState::new(100, 300);
        assert!(st.is_scrollable());
        assert_eq!(st.max_scroll(), 200);

        // Scroll down by 1 wheel notch (-120) with 40px step
        st.apply_wheel_delta(-120, 40);
        assert_eq!(st.scroll_offset, 40);

        // Scroll down past max -> clamps to 200
        st.scroll_by(500);
        assert_eq!(st.scroll_offset, 200);

        // Scroll up past 0 -> clamps to 0
        st.scroll_by(-500);
        assert_eq!(st.scroll_offset, 0);

        // Proportional scrollbar thumb calculation
        let track = LayoutRect::new(294, 0, 4, 90);
        let thumb_top = st.thumb_rect(track, 20).expect("Should produce thumb");
        assert_eq!(thumb_top.height, 30); // 100/300 * 90 = 30
        assert_eq!(thumb_top.y, 0);

        st.scroll_by(200); // bottom of scroll range
        let thumb_bottom = st.thumb_rect(track, 20).expect("Should produce thumb");
        assert_eq!(thumb_bottom.y, 60); // 90 - 30 = 60
    }

    #[test]
    fn test_scroll_view_layout_and_target_clipping() {
        let scale = DpiScale { dpi: 96 };
        let fonts = LayoutFonts {
            title: HFONT(0 as _),
            semibold: HFONT(0 as _),
            normal: HFONT(0 as _),
            small: HFONT(0 as _),
        };
        let hdc = HDC(0 as _);

        // Create 5 buttons of 30px height with 10px gap -> 5 * 30 + 4 * 10 = 190px total content height
        let mut vstack = LayoutNode::vstack().gap(10);
        for i in 0..5 {
            vstack = vstack.child(LayoutNode::button(
                format!("Item {}", i),
                Length::Fixed(200),
                30,
                FlyoutAction::TestHud,
            ));
        }

        // ScrollView with 80px viewport height and scroll_offset = 40px (skips Item 0 completely)
        let scroll = LayoutNode::scroll_view(80, vstack.build())
            .scroll_offset(40)
            .build();

        let measured = scroll.measure(300, 500, &scale, &fonts, hdc);
        assert_eq!(measured.height, 80);

        let viewport_bounds = LayoutRect::new(0, 100, 300, 80);
        let placed = scroll.layout(viewport_bounds, &scale, &fonts, hdc);

        if let PlacedNode::ScrollView { content_height, scroll_offset, .. } = &placed {
            assert_eq!(*content_height, 190);
            assert_eq!(*scroll_offset, 40);
        } else {
            panic!("Expected PlacedNode::ScrollView");
        }

        let mut targets = Vec::new();
        let mut tooltips = Vec::new();
        unsafe {
            placed.render(hdc, &scale, &fonts, None, &mut targets, &mut tooltips);
        }

        // Item 0 is at y: 60..90 (above viewport 100..180 -> clipped out!)
        // Item 1 is at y: 100..130 (inside viewport -> visible)
        // Item 2 is at y: 140..170 (inside viewport -> visible)
        // Item 3 is at y: 180..210 (starts at 180 -> outside viewport 100..180 -> clipped out!)
        // Item 4 is at y: 220..250 (outside viewport -> clipped out!)
        assert_eq!(targets.len(), 2);
        assert_eq!(targets[0].rect.top, 100);
        assert_eq!(targets[0].rect.bottom, 130);
        assert_eq!(targets[1].rect.top, 140);
        assert_eq!(targets[1].rect.bottom, 170);
    }
}
