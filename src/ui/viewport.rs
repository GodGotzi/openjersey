//! The floating toolbar in the 3D view: camera views, which garments are
//! shown, and snapping.

use myth::egui::egui::{self, Align2, Color32, CornerRadius, Margin, RichText, Shadow, Stroke};

use crate::i18n::tr;
use crate::scene::{Studio, View};
use crate::workspace::Workspace;

const OFFSET_FROM_TOP: f32 = 12.0;
/// Free space kept on each side of the bar.
const SIDE_MARGIN: f32 = 12.0;
/// The bar's width before it has been shown once.
const DEFAULT_WIDTH: f32 = 340.0;

/// Shows the bar when the pane is wide enough for it; it is left out
/// otherwise rather than squeezed or overlapping the side panels.
pub fn toolbar(ctx: &egui::Context, pane: egui::Rect, ws: &mut Workspace, studio: &mut Studio) {
    let id = egui::Id::new("viewport-toolbar");
    let width = ctx.data(|d| d.get_temp::<f32>(id)).unwrap_or(DEFAULT_WIDTH);
    if pane.width() < width + 2.0 * SIDE_MARGIN {
        return;
    }
    let shown = egui::Area::new(id)
        .pivot(Align2::CENTER_TOP)
        .fixed_pos(pane.center_top() + egui::vec2(0.0, OFFSET_FROM_TOP))
        .order(egui::Order::Foreground)
        .show(ctx, |ui| {
            let visuals = ui.visuals();
            let frame = egui::Frame::new()
                .fill(visuals.window_fill.gamma_multiply(0.92))
                .stroke(Stroke::new(1.0, visuals.widgets.noninteractive.bg_stroke.color))
                .corner_radius(CornerRadius::same(16))
                .inner_margin(Margin::symmetric(8, 5))
                .shadow(Shadow { offset: [0, 2], blur: 10, spread: 0, color: Color32::from_black_alpha(40) });
            frame.show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 3.0;
                    views(ui, studio);
                    ui.separator();
                    garments(ui, ws);
                    ui.separator();
                    let hint =
                        tr("Snap to centre lines, pieces and other objects", "Einrasten an Mittellinien, Teilen und anderen Objekten");
                    ui.toggle_value(&mut ws.snap, tr("Snap", "Einrasten")).on_hover_text(hint);
                });
            });
        });
    ctx.data_mut(|d| d.insert_temp(id, shown.response.rect.width()));
}

fn views(ui: &mut egui::Ui, studio: &mut Studio) {
    let buttons = [
        (View::Front, tr("Front", "Vorne")),
        (View::Left, tr("Left", "Links")),
        (View::Back, tr("Back", "Hinten")),
        (View::Right, tr("Right", "Rechts")),
    ];
    for (view, label) in buttons {
        if ui
            .button(RichText::new(label))
            .on_hover_text(tr("Look at the model from this side", "Modell von dieser Seite ansehen"))
            .clicked()
        {
            studio.set_view(view);
        }
    }
}

/// Show or hide the top and the bottom (a hidden one is also left off the
/// print sheet).
fn garments(ui: &mut egui::Ui, ws: &mut Workspace) {
    let mut shown = ws.doc.garments;
    let hint = tr(
        "Show or hide (hidden garments are left off the print sheet)",
        "Ein- oder ausblenden (ausgeblendete Teile fehlen auf der Druckvorlage)",
    );
    ui.toggle_value(&mut shown.shirt, tr("Top", "Oberteil")).on_hover_text(hint);
    ui.toggle_value(&mut shown.shorts, tr("Bottom", "Unterteil")).on_hover_text(hint);
    if shown != ws.doc.garments {
        ws.checkpoint();
        ws.doc.garments = shown;
        ws.changed();
    }
}
