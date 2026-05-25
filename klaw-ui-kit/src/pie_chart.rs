use std::f32::consts::{FRAC_PI_2, TAU};

/// One data segment in a [`PieChart`].
#[derive(Debug, Clone, PartialEq)]
pub struct PieSlice {
    pub label: String,
    pub value: f32,
    pub color: Option<egui::Color32>,
}

impl PieSlice {
    #[must_use]
    pub fn new(label: impl Into<String>, value: f32) -> Self {
        Self {
            label: label.into(),
            value,
            color: None,
        }
    }

    #[must_use]
    pub const fn color(mut self, color: egui::Color32) -> Self {
        self.color = Some(color);
        self
    }
}

/// Color palette used for slices that do not provide an explicit color.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PieChartPalette {
    #[default]
    Tableau,
    Rainbow,
    Blues,
    Grays,
}

impl PieChartPalette {
    #[must_use]
    pub fn slice_color(self, index: usize, total: usize) -> egui::Color32 {
        match self {
            Self::Tableau => tableau_color(index, total),
            Self::Rainbow => {
                let hue = index as f32 / total.max(1) as f32;
                egui::epaint::Hsva::new(hue, 0.9, 0.9, 1.0).into()
            }
            Self::Blues => {
                let denominator = total.saturating_sub(1).max(1) as f32;
                let intensity = 0.3 + 0.7 * (index as f32 / denominator);
                egui::Color32::from_rgb(
                    (intensity * 0.2 * 255.0) as u8,
                    (intensity * 0.6 * 255.0) as u8,
                    (intensity * 0.9 * 255.0) as u8,
                )
            }
            Self::Grays => {
                let intensity = 200 - (index as u8 * 20 % 150);
                egui::Color32::from_rgb(intensity, intensity, intensity)
            }
        }
    }
}

/// Shared `egui` pie chart widget.
///
/// The drawing strategy is adapted from `pie_egui` and updated for this
/// workspace's `egui` version and shared widget API.
#[derive(Debug, Clone, Copy)]
pub struct PieChart<'a> {
    slices: &'a [PieSlice],
    palette: PieChartPalette,
    show_outline: bool,
    show_separators: bool,
    show_labels: bool,
    desired_size: Option<egui::Vec2>,
}

impl<'a> PieChart<'a> {
    #[must_use]
    pub const fn new(slices: &'a [PieSlice]) -> Self {
        Self {
            slices,
            palette: PieChartPalette::Tableau,
            show_outline: true,
            show_separators: true,
            show_labels: true,
            desired_size: None,
        }
    }

    #[must_use]
    pub const fn palette(mut self, palette: PieChartPalette) -> Self {
        self.palette = palette;
        self
    }

    #[must_use]
    pub const fn show_outline(mut self, show_outline: bool) -> Self {
        self.show_outline = show_outline;
        self
    }

    #[must_use]
    pub const fn show_separators(mut self, show_separators: bool) -> Self {
        self.show_separators = show_separators;
        self
    }

    #[must_use]
    pub const fn show_labels(mut self, show_labels: bool) -> Self {
        self.show_labels = show_labels;
        self
    }

    #[must_use]
    pub const fn desired_size(mut self, desired_size: egui::Vec2) -> Self {
        self.desired_size = Some(desired_size);
        self
    }

    fn resolved_size(self, ui: &egui::Ui) -> egui::Vec2 {
        self.desired_size.unwrap_or_else(|| {
            let available = ui.available_size();
            let side = available.x.min(available.y);
            let side = if side.is_finite() && side > 0.0 {
                side
            } else {
                160.0
            };
            egui::Vec2::splat(side)
        })
    }
}

impl egui::Widget for PieChart<'_> {
    fn ui(self, ui: &mut egui::Ui) -> egui::Response {
        let desired_size = self.resolved_size(ui);
        let (rect, response) = ui.allocate_exact_size(desired_size, egui::Sense::hover());

        if ui.is_rect_visible(rect) {
            let slices = normalized_slices(self.slices, self.palette);
            if !slices.is_empty() {
                paint_pie_chart(ui, rect, &slices, self);
            }
        }

        response
    }
}

/// Build equal-value slices labeled from 1 to `count`.
#[must_use]
pub fn equal_pie_slices(count: usize) -> Vec<PieSlice> {
    (1..=count)
        .map(|index| PieSlice::new(index.to_string(), 1.0))
        .collect()
}

#[derive(Debug, Clone, PartialEq)]
struct NormalizedSlice {
    label: String,
    fraction: f32,
    color: egui::Color32,
}

fn normalized_slices(slices: &[PieSlice], palette: PieChartPalette) -> Vec<NormalizedSlice> {
    let valid_slices = slices
        .iter()
        .filter(|slice| slice.value.is_finite() && slice.value > 0.0)
        .collect::<Vec<_>>();
    let total_value = valid_slices.iter().map(|slice| slice.value).sum::<f32>();

    if total_value <= 0.0 || !total_value.is_finite() {
        return Vec::new();
    }

    let total_slices = valid_slices.len();
    valid_slices
        .into_iter()
        .enumerate()
        .map(|(index, slice)| NormalizedSlice {
            label: slice.label.clone(),
            fraction: slice.value / total_value,
            color: slice
                .color
                .unwrap_or_else(|| palette.slice_color(index, total_slices)),
        })
        .collect()
}

fn paint_pie_chart(
    ui: &egui::Ui,
    rect: egui::Rect,
    slices: &[NormalizedSlice],
    chart: PieChart<'_>,
) {
    let side = rect.width().min(rect.height());
    let chart_rect = egui::Rect::from_center_size(rect.center(), egui::Vec2::splat(side));
    let painter = ui.painter_at(rect);
    let center = chart_rect.center();
    let radius = side * if chart.show_labels { 0.38 } else { 0.46 };
    let separator_stroke =
        egui::Stroke::new(1.0, ui.visuals().widgets.noninteractive.bg_stroke.color);
    let outline_stroke =
        egui::Stroke::new(1.5, ui.visuals().widgets.noninteractive.bg_stroke.color);

    if slices.len() == 1 {
        painter.circle_filled(center, radius, slices[0].color);
        if chart.show_labels {
            paint_label(ui, &painter, center, radius, FRAC_PI_2, &slices[0].label);
        }
    } else {
        let mut start_angle = -FRAC_PI_2;
        for slice in slices {
            let sweep = slice.fraction * TAU;
            let end_angle = start_angle + sweep;
            paint_slice(
                &painter,
                center,
                radius,
                start_angle,
                end_angle,
                slice.color,
            );

            if chart.show_separators {
                paint_radial_line(&painter, center, radius, start_angle, separator_stroke);
            }
            if chart.show_labels {
                paint_label(
                    ui,
                    &painter,
                    center,
                    radius,
                    start_angle + sweep * 0.5,
                    &slice.label,
                );
            }

            start_angle = end_angle;
        }
    }

    if chart.show_outline {
        painter.circle_stroke(center, radius, outline_stroke);
    }
}

fn paint_slice(
    painter: &egui::Painter,
    center: egui::Pos2,
    radius: f32,
    start_angle: f32,
    end_angle: f32,
    color: egui::Color32,
) {
    let sweep = end_angle - start_angle;
    let steps = ((radius * sweep.abs()) / 8.0).ceil().clamp(2.0, 64.0) as usize;
    let mut points = Vec::with_capacity(steps + 2);
    points.push(center);

    for step in 0..=steps {
        let t = step as f32 / steps as f32;
        let angle = start_angle + t * sweep;
        points.push(egui::pos2(
            center.x + radius * angle.cos(),
            center.y + radius * angle.sin(),
        ));
    }

    painter.add(egui::Shape::convex_polygon(
        points,
        color,
        egui::Stroke::NONE,
    ));
}

fn paint_radial_line(
    painter: &egui::Painter,
    center: egui::Pos2,
    radius: f32,
    angle: f32,
    stroke: egui::Stroke,
) {
    let end = egui::pos2(
        center.x + radius * angle.cos(),
        center.y + radius * angle.sin(),
    );
    painter.line_segment([center, end], stroke);
}

fn paint_label(
    ui: &egui::Ui,
    painter: &egui::Painter,
    center: egui::Pos2,
    radius: f32,
    angle: f32,
    label: &str,
) {
    if label.is_empty() {
        return;
    }

    let label_distance = radius + ui.spacing().interact_size.y * 0.45;
    let position = egui::pos2(
        center.x + label_distance * angle.cos(),
        center.y + label_distance * angle.sin(),
    );
    painter.text(
        position,
        egui::Align2::CENTER_CENTER,
        label,
        egui::FontId::proportional(ui.text_style_height(&egui::TextStyle::Body) * 0.85),
        ui.visuals().text_color(),
    );
}

fn tableau_color(index: usize, total: usize) -> egui::Color32 {
    const PALETTE: [egui::Color32; 10] = [
        egui::Color32::from_rgb(87, 120, 164),
        egui::Color32::from_rgb(228, 148, 68),
        egui::Color32::from_rgb(209, 97, 93),
        egui::Color32::from_rgb(133, 182, 178),
        egui::Color32::from_rgb(106, 159, 88),
        egui::Color32::from_rgb(231, 202, 96),
        egui::Color32::from_rgb(168, 124, 159),
        egui::Color32::from_rgb(241, 162, 169),
        egui::Color32::from_rgb(150, 118, 98),
        egui::Color32::from_rgb(184, 176, 172),
    ];

    if total > 1 && index == total - 1 && (index + 1) % PALETTE.len() == 1 {
        PALETTE[(index + 1) % PALETTE.len()]
    } else {
        PALETTE[index % PALETTE.len()]
    }
}

#[cfg(test)]
mod tests {
    use super::{PieChartPalette, PieSlice, equal_pie_slices, normalized_slices};

    #[test]
    fn palette_returns_stable_colors() {
        assert_eq!(
            PieChartPalette::Tableau.slice_color(0, 3),
            PieChartPalette::Tableau.slice_color(0, 3)
        );
        assert_eq!(
            PieChartPalette::Rainbow.slice_color(2, 5),
            PieChartPalette::Rainbow.slice_color(2, 5)
        );
        assert_eq!(
            PieChartPalette::Blues.slice_color(1, 4),
            PieChartPalette::Blues.slice_color(1, 4)
        );
        assert_eq!(
            PieChartPalette::Grays.slice_color(3, 6),
            PieChartPalette::Grays.slice_color(3, 6)
        );
    }

    #[test]
    fn equal_pie_slices_builds_unit_slices() {
        assert!(equal_pie_slices(0).is_empty());

        let slices = equal_pie_slices(3);
        assert_eq!(slices.len(), 3);
        assert_eq!(slices[0], PieSlice::new("1", 1.0));
        assert_eq!(slices[1], PieSlice::new("2", 1.0));
        assert_eq!(slices[2], PieSlice::new("3", 1.0));
    }

    #[test]
    fn normalized_slices_filters_non_positive_and_non_finite_values() {
        let slices = vec![
            PieSlice::new("valid", 2.0),
            PieSlice::new("zero", 0.0),
            PieSlice::new("negative", -1.0),
            PieSlice::new("nan", f32::NAN),
            PieSlice::new("infinite", f32::INFINITY),
            PieSlice::new("also valid", 6.0),
        ];

        let normalized = normalized_slices(&slices, PieChartPalette::Tableau);

        assert_eq!(normalized.len(), 2);
        assert_eq!(normalized[0].label, "valid");
        assert_eq!(normalized[0].fraction, 0.25);
        assert_eq!(normalized[1].label, "also valid");
        assert_eq!(normalized[1].fraction, 0.75);
    }

    #[test]
    fn normalized_slices_returns_empty_when_no_valid_values_exist() {
        let slices = vec![PieSlice::new("zero", 0.0), PieSlice::new("negative", -1.0)];

        assert!(normalized_slices(&slices, PieChartPalette::Tableau).is_empty());
    }
}
