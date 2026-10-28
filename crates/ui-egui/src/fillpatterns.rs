//! Manage > Fill Patterns, as Revit's dialogs: the project's filled region types (a foreground and a background pattern,
//! their colours, masking or not) and its fill patterns (families of lines, drafting or model), on two tabs of one draft,
//! applied as one undo step (OK and Apply), or dropped (Cancel). A pattern of one family, or two square to each other, is
//! edited as Revit's simple pattern: its angle, its spacing, crosshatched or not; any pattern's families, one by one, with
//! Edit Lines (Revit's custom patterns, which it only imports). Patterns come in from a PAT file and go out to one.

use bimcraft_i18n::msg;

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use bimcraft_core::engine::Session;
use bimcraft_core::fills::{self, FillPattern, FilledRegionType, HatchLine, MAX_SPACING, MIN_SPACING, Target};
use bimcraft_core::geom::P2;
use bimcraft_core::model::{Id, Model};
use bimcraft_core::render::{PAPER, Rgb};
use egui::{
    Align, Button, Checkbox, Color32, ComboBox, Context, DragValue, Grid, Id as UiId, Layout, Modal, RichText, ScrollArea, Sense, Stroke,
    TextEdit, Ui, WidgetInfo, WidgetType, pos2, vec2,
};
use serde_json::{Value, json};

use crate::lang::{t, tf};
use crate::renames::{self, Row};
use crate::style::{ERROR_HEIGHT, boxed_text, canvas_pal, dialog_room, dialog_row, pal};
use crate::{BimApp, Status};

/// The dialog's tabs.
const TABS: [&str; 2] = [msg!("Region Types"), "Patterns"];

/// A pattern as edited: the name it had (none, new), the name it has, what it is.
struct PatternRow {
    was: Option<String>,
    name: String,
    pattern: FillPattern,
}

/// A type as edited: the name it had, the name it has, its patterns as their rows (none, none), its colours, masking.
#[derive(Clone)]
struct TypeRow {
    was: Option<String>,
    name: String,
    foreground: Option<usize>,
    foreground_color: [u8; 3],
    background: Option<usize>,
    background_color: [u8; 3],
    masking: bool,
}

/// The Fill Patterns dialog: its tab, the types and patterns as edited.
pub(crate) struct FillPatternsDialog {
    tab: usize,
    types: Vec<TypeRow>,
    patterns: Vec<PatternRow>,
    /// Patterns deleted: their rows stay, so the types' indices do, and are skipped.
    gone: Vec<bool>,
    /// The pattern whose families Edit Lines shows, and each family's dashes as typed.
    editing: Option<(usize, Vec<String>)>,
    /// Edit Type's current row and the instances to assign it to; None opens Manage's table.
    subject: Option<(usize, Vec<Id>)>,
    error: Option<String>,
    /// What the last import or export said, and whether something went wrong or was left out.
    note: Option<(String, bool)>,
    /// Whether the patterns of a file replace those of the same name in the draft; else they are left out.
    replace: bool,
    /// The patterns and types the project held when this opened: a draft is not applied over a project that has changed since.
    held: (BTreeMap<String, FillPattern>, BTreeMap<String, FilledRegionType>),
}

/// What the Patterns tab's buttons ask of the app: a file to import from, or to export to.
#[derive(Clone, Copy, PartialEq)]
enum FileAsk {
    Import,
    Export,
}

impl FillPatternsDialog {
    /// Open on the project's region types and fill patterns.
    pub(crate) fn new(m: &Model) -> FillPatternsDialog {
        let patterns: Vec<PatternRow> =
            m.fill_patterns.iter().map(|(n, p)| PatternRow { was: Some(n.clone()), name: n.clone(), pattern: p.clone() }).collect();
        let row = |n: &Option<String>| n.as_ref().and_then(|n| patterns.iter().position(|r| r.name == *n));
        let types = (m.region_types.iter())
            .map(|(n, x)| TypeRow {
                was: Some(n.clone()),
                name: n.clone(),
                foreground: row(&x.foreground),
                foreground_color: x.foreground_color,
                background: row(&x.background),
                background_color: x.background_color,
                masking: x.masking,
            })
            .collect();
        let gone = vec![false; patterns.len()];
        let held = (m.fill_patterns.clone(), m.region_types.clone());
        FillPatternsDialog { tab: 0, types, patterns, gone, editing: None, subject: None, error: None, note: None, replace: false, held }
    }

    /// The patterns a PAT file held, added to the draft (one of a name it has already takes its place).
    fn import(&mut self, patterns: Vec<(String, FillPattern)>) {
        for (name, pattern) in patterns {
            match self.patterns.iter().enumerate().position(|(i, r)| !self.gone[i] && r.name == name) {
                Some(i) => self.patterns[i].pattern = pattern,
                None => {
                    self.patterns.push(PatternRow { was: None, name, pattern });
                    self.gone.push(false);
                }
            }
        }
    }

    /// The PAT file at `path`, read by the `import_fill_patterns` command, so that one code path reads, caps and keeps or
    /// replaces; it runs on a scratch project that holds the names of the draft's patterns, whose own keep what it brings.
    /// What came into the draft and what was left out, said.
    fn read(&mut self, path: &Path) -> Result<(String, bool), String> {
        let mut scratch = Session::new();
        let m = scratch.model_mut();
        m.fill_patterns =
            self.live().map(|(_, r)| r.name.trim()).filter(|n| !n.is_empty()).map(|n| (n.to_string(), FillPattern::default())).collect();
        m.region_types.clear();
        for x in m.materials.values_mut() {
            (x.cut_pattern, x.surface_pattern) = (None, None);
        }
        let reply = scratch.execute("import_fill_patterns", &json!({"path": path.to_string_lossy(), "replace": self.replace}))?;
        let names: Vec<&str> = reply["imported"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
        let brought = names.iter().filter_map(|n| scratch.model.fill_patterns.get(*n).map(|p| (n.to_string(), p.clone()))).collect();
        self.import(brought);
        let out: Vec<String> = (reply["left_out"].as_array().into_iter().flatten())
            .map(|l| {
                let name = l["name"].as_str().unwrap_or_default();
                format!("{} ({})", short(name, 30), short(reason(name, l["why"].as_str().unwrap_or_default()), 70))
            })
            .collect();
        let n = names.len().to_string();
        let one = names.len() == 1;
        // The reply names the first few it left out and counts them all.
        let left = reply["left_out_count"].as_u64().map_or(0, |c| usize::try_from(c).unwrap_or(usize::MAX)).max(out.len());
        Ok(match left {
            0 => (tf(if one { msg!("{} pattern imported.") } else { msg!("{} patterns imported.") }, &[&n]), false),
            k => {
                let list = out.iter().take(LEFT_OUT_SHOWN).cloned().collect::<Vec<_>>().join("; ");
                let list = if k > LEFT_OUT_SHOWN { tf("{}; and {} more", &[&list, &(k - LEFT_OUT_SHOWN).to_string()]) } else { list };
                (
                    tf(
                        if one { msg!("{} pattern imported; left out: {}") } else { msg!("{} patterns imported; left out: {}") },
                        &[&n, &list],
                    ),
                    true,
                )
            }
        })
    }

    /// The patterns as they are in the draft, for a PAT file.
    fn drafted(&self) -> Vec<(String, FillPattern)> {
        self.live().map(|(_, r)| (r.name.clone(), r.pattern.clone())).collect()
    }

    /// Open Edit Type for a region being placed or selected.
    pub(crate) fn edit(m: &Model, name: &str, elements: Vec<Id>) -> Self {
        let mut d = Self::new(m);
        let row = d.types.iter().position(|r| r.name == name).unwrap_or(0);
        d.subject = Some((row, elements));
        d
    }

    /// What stops the draft being applied: a project that changed since it was opened (an agent's command, say), whose
    /// patterns and types the draft would undo or delete unseen; or a name left blank or used twice.
    fn check(&self, m: &Model) -> Result<(), String> {
        if self.held.0 != m.fill_patterns || self.held.1 != m.region_types {
            return Err(t("The project changed while this window was open: Cancel, and open it again.").to_string());
        }
        self.validate_names()
    }

    /// Refuse name collisions before commands can accidentally update an existing definition.
    fn validate_names(&self) -> Result<(), String> {
        for names in [self.types.iter().map(|r| r.name.trim()).collect::<Vec<_>>(), self.live().map(|(_, r)| r.name.trim()).collect()] {
            let mut seen = std::collections::BTreeSet::new();
            for n in names {
                if n.is_empty() {
                    return Err(t("Enter a name for every type and pattern.").to_string());
                }
                if !seen.insert(n) {
                    return Err(tf("The name `{}` is already in use.", &[n]));
                }
            }
        }
        Ok(())
    }

    /// The patterns not deleted, with their rows.
    fn live(&self) -> impl Iterator<Item = (usize, &PatternRow)> {
        self.patterns.iter().enumerate().filter(|(i, _)| !self.gone[*i])
    }

    /// A type for New Type, in either view of the dialog: a name not in use, filled solid black as Revit's new types are.
    fn new_type(&self) -> TypeRow {
        let taken: Vec<&str> = self.types.iter().map(|s| s.name.as_str()).collect();
        let solid = self.live().find(|(_, p)| p.name.as_str() == t(fills::SOLID_FILL) || p.name == fills::SOLID_FILL).map(|(i, _)| i);
        TypeRow {
            was: None,
            name: fresh(t("Region Type"), &taken),
            foreground: solid,
            foreground_color: [0, 0, 0],
            background: None,
            background_color: [255, 255, 255],
            masking: false,
        }
    }

    /// A type as the project would hold it, its patterns by their names in the draft.
    fn region_type(&self, r: &TypeRow) -> FilledRegionType {
        let name = |i: Option<usize>| i.and_then(|i| self.patterns.get(i)).map(|p| p.name.clone());
        FilledRegionType {
            foreground: name(r.foreground),
            foreground_color: r.foreground_color,
            background: name(r.background),
            background_color: r.background_color,
            masking: r.masking,
        }
    }
}

/// How many of the patterns a file left out the dialog names; the rest it counts.
const LEFT_OUT_SHOWN: usize = 5;

/// `text` cut to at most `most` characters at a word's end, as a name or a reason is told in a message: a word cut in two is a
/// reason lost (a text of one word is cut where it must be).
fn short(text: &str, most: usize) -> String {
    let Some((end, _)) = text.char_indices().nth(most) else { return text.to_string() };
    let cut = &text[..end];
    let cut =
        if text[end..].starts_with(char::is_whitespace) { cut } else { cut.rsplit_once(char::is_whitespace).map_or(cut, |(head, _)| head) };
    format!("{}…", cut.trim_end())
}

/// Why the core left pattern `name` out, without the pattern's name it begins with (the message names it, and the dialog does).
fn reason<'a>(name: &str, why: &'a str) -> &'a str {
    why.strip_prefix(&format!("fill pattern `{name}`")).map_or(why, |rest| rest.trim_start_matches([':', ' ']))
}

/// `base` and a number, the first that `taken` hasn't: `Pattern 1`, `Pattern 2`…
fn fresh(base: &str, taken: &[&str]) -> String {
    (1..).map(|i| format!("{base} {i}")).find(|n| !taken.contains(&n.as_str())).unwrap_or_default()
}

/// A text field named `name` for screen readers.
fn name_field(ui: &mut Ui, text: &mut String, name: String) {
    let r = ui.add_sized(vec2(150.0, 20.0), TextEdit::singleline(text));
    r.widget_info(|| WidgetInfo::labeled(WidgetType::TextEdit, true, name.clone()));
}

/// A small button named `name` for screen readers, showing `text`.
fn named_button(ui: &mut Ui, text: &str, name: String) -> bool {
    let r = ui.add(Button::new(text).small());
    r.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, name.clone()));
    r.clicked()
}

/// A colour button named `name`: red, green and blue only, since the alpha slider of an RGBA button premultiplies them.
fn colour_field(ui: &mut Ui, rgb: &mut [u8; 3], name: String) {
    let r = ui.color_edit_button_srgb(rgb);
    r.widget_info(|| WidgetInfo::labeled(WidgetType::ColorButton, true, name.clone()));
}

/// A swatch of `p`, `width` x 22 px, in `colour`: a drafting pattern at 5 px to its mm, a model one at a tenth of a px. A
/// `colour` that is the canvas's (ink as the plan draws it) is shown on the canvas's `paper`: in a dark interface with a light
/// canvas, black ink would be lost on the interface's own ground.
pub(crate) fn swatch(ui: &mut Ui, p: Option<&FillPattern>, width: f32, colour: Color32, paper: Option<Color32>) {
    let (rect, _) = ui.allocate_exact_size(vec2(width, 22.0), Sense::hover());
    if let Some(paper) = paper {
        ui.painter().rect_filled(rect, 2.0, paper);
    }
    paint_pattern(ui, rect, p, colour);
    ui.painter().rect_stroke(rect, 2.0, Stroke::new(1.0, ui.visuals().widgets.noninteractive.bg_stroke.color), egui::StrokeKind::Inside);
}

pub(crate) fn paint_pattern(ui: &Ui, rect: egui::Rect, p: Option<&FillPattern>, colour: Color32) {
    // Off the screen (a scroll area's rows past its edge) there is nothing to paint, and a long list is not worked out each frame.
    if !ui.is_rect_visible(rect) {
        return;
    }
    let painter = ui.painter_at(rect);
    let Some(p) = p else { return };
    if p.is_solid() {
        painter.rect_filled(rect, 0.0, colour);
        return;
    }
    let px = if p.target == Target::Model { 0.1 } else { 5.0 };
    let (w, h) = (f64::from(rect.width()) / px, f64::from(rect.height()) / px);
    let ring = vec![P2(0.0, 0.0), P2(w, 0.0), P2(w, h), P2(0.0, h)];
    let strokes = fills::strokes(p, std::slice::from_ref(&ring), 1.0, (P2(0.0, 0.0), P2(w, h)), 4000).unwrap_or_default();
    let at = |q: P2| pos2(rect.left() + (q.0 * px) as f32, rect.bottom() - (q.1 * px) as f32);
    for [a, b] in strokes {
        if a.dist(b) * px < 0.5 {
            painter.circle_filled(at(a), 0.7, colour);
        } else {
            painter.line_segment([at(a), at(b)], Stroke::new(1.0, colour));
        }
    }
}

/// A drop-down of the patterns (or none), named `label`, `row` the one picked.
fn pattern_pick(ui: &mut Ui, row: &mut Option<usize>, names: &[(usize, String)], label: String) {
    let shown = row.and_then(|p| names.iter().find(|(i, _)| *i == p)).map_or_else(|| t("<None>").to_string(), |(_, n)| n.clone());
    let c = ComboBox::from_id_salt(&label).selected_text(shown).width(150.0).show_ui(ui, |ui| {
        ui.selectable_value(row, None, t("<None>"));
        for (p, n) in names {
            ui.selectable_value(row, Some(*p), n);
        }
    });
    c.response.widget_info(|| WidgetInfo::labeled(WidgetType::ComboBox, true, label.clone()));
}

/// The types: a row each, its name, its foreground and background (pattern, colour, swatch), masking, Delete; New Type. The
/// rows scroll in what the window has left over `below`, the dialog's own notes and buttons.
fn types_tab(ui: &mut Ui, d: &mut FillPatternsDialog, below: f32) {
    ui.label(t("What filled regions are filled with: a foreground pattern over a background one, over the paper when it masks."));
    ui.add_space(4.0);
    let names: Vec<(usize, String)> = d.live().map(|(i, p)| (i, p.name.clone())).collect();
    let patterns: Vec<FillPattern> = d.patterns.iter().map(|p| p.pattern.clone()).collect();
    let mut delete = None;
    ScrollArea::vertical().max_height(dialog_room(ui, below + dialog_row(ui))).show(ui, |ui| {
        Grid::new("region-types").striped(true).num_columns(9).show(ui, |ui| {
            for h in ["Name", msg!("Foreground"), "", "", msg!("Background"), "", "", "Masking", ""] {
                ui.label(RichText::new(t(h)).strong());
            }
            ui.end_row();
            for (i, r) in d.types.iter_mut().enumerate() {
                let who = r.name.clone();
                name_field(ui, &mut r.name, tf("{}: Name", &[&who]));
                for (row, rgb, side) in
                    [(&mut r.foreground, &mut r.foreground_color, "Foreground"), (&mut r.background, &mut r.background_color, "Background")]
                {
                    pattern_pick(ui, row, &names, tf("{}: {}", &[&who, t(side)]));
                    colour_field(ui, rgb, tf("{}: {} Colour", &[&who, t(side)]));
                    // As the plan draws it, on its paper: black ink is light on a dark ground.
                    let ink = crate::canvas::color(bimcraft_core::render::Rgb(rgb[0], rgb[1], rgb[2]));
                    swatch(ui, row.and_then(|p| patterns.get(p)), 60.0, ink, Some(crate::canvas::color(PAPER)));
                }
                let c = ui.add(Checkbox::without_text(&mut r.masking));
                c.widget_info(|| WidgetInfo::selected(WidgetType::Checkbox, true, r.masking, tf("{}: Masking", &[&who])));
                if named_button(ui, t("Delete"), tf("{}: Delete", &[&who])) {
                    delete = Some(i);
                }
                ui.end_row();
            }
        });
    });
    if let Some(i) = delete {
        d.types.remove(i);
    }
    if ui.button(t("New Type")).clicked() {
        d.types.push(d.new_type());
    }
}

/// A family's dashes as typed: lengths, a space's negative, 0 a dot, between commas.
fn dashes_text(d: &[f64]) -> String {
    d.iter().map(|x| format!("{}", x + 0.0)).collect::<Vec<_>>().join(", ")
}

/// The families of pattern `i`, one a row: its angle, origin, shift along, spacing across and dashes; Delete; Add
/// Family. Dashes that don't read as lengths are left as they were until they do. The rows scroll within `room`.
fn families(ui: &mut Ui, d: &mut FillPatternsDialog, room: f32) {
    let Some((i, texts)) = &mut d.editing else { return };
    let Some(r) = d.patterns.get_mut(*i) else { return };
    let who = r.name.clone();
    ui.add_space(6.0);
    ui.label(RichText::new(tf("Line families of {}", &[&who])).strong());
    texts.resize(r.pattern.lines.len(), String::new());
    let mut delete = None;
    // The heading above and the row of buttons below are not the rows' room.
    let rows = (room - 2.0 * dialog_row(ui) - 6.0).max(60.0);
    ScrollArea::vertical().id_salt("pattern-families-rows").max_height(rows).show(ui, |ui| {
        Grid::new("pattern-families").striped(true).num_columns(8).show(ui, |ui| {
            for h in ["", "Angle", msg!("X"), msg!("Y"), msg!("Shift"), msg!("Spacing"), msg!("Dashes"), ""] {
                ui.label(RichText::new(t(h)).strong());
            }
            ui.end_row();
            for (k, (l, text)) in r.pattern.lines.iter_mut().zip(texts.iter_mut()).enumerate() {
                let n = (k + 1).to_string();
                ui.label(&n);
                let name = |what: &str| tf("{} family {}: {}", &[&who, &n, t(what)]);
                for (what, v, range) in [
                    ("Angle", &mut l.angle, -360.0..=360.0),
                    ("X", &mut l.origin.0, -MAX_SPACING..=MAX_SPACING),
                    ("Y", &mut l.origin.1, -MAX_SPACING..=MAX_SPACING),
                    ("Shift", &mut l.shift, -MAX_SPACING..=MAX_SPACING),
                    ("Spacing", &mut l.spacing, MIN_SPACING..=MAX_SPACING),
                ] {
                    let label = name(what);
                    let r = ui.add(DragValue::new(v).range(range).speed(0.1).max_decimals(3));
                    r.widget_info(|| WidgetInfo::labeled(WidgetType::DragValue, true, label.clone()));
                }
                let label = name("Dashes");
                let e = ui.add_sized(vec2(160.0, 20.0), TextEdit::singleline(text).hint_text(t("whole")));
                e.widget_info(|| WidgetInfo::labeled(WidgetType::TextEdit, true, label.clone()));
                if e.changed() {
                    let read: Result<Vec<f64>, _> =
                        text.split(',').map(str::trim).filter(|x| !x.is_empty()).map(|x| x.parse::<f64>()).collect();
                    if let Ok(dashes) = read.map(|v| v.into_iter().filter(|x| x.is_finite()).collect::<Vec<_>>()) {
                        l.dashes = dashes;
                    }
                }
                if named_button(ui, t("Delete"), name("Delete")) {
                    delete = Some(k);
                }
                ui.end_row();
            }
        });
    });
    if let Some(k) = delete {
        r.pattern.lines.remove(k);
        texts.remove(k);
    }
    let mut done = false;
    ui.horizontal(|ui| {
        if ui.button(t("Add Family")).clicked() && r.pattern.lines.len() < fills::MAX_FAMILIES {
            r.pattern.lines.push(HatchLine::simple(0.0, if r.pattern.target == Target::Model { 100.0 } else { 2.0 }));
            texts.push(String::new());
        }
        done = ui.button(t("Done")).clicked();
    });
    if done {
        d.editing = None;
    }
}

/// Edit one region type, as in Revit's Type Properties; all edits stay in the draft until Apply or OK.
fn edit_type(ui: &mut Ui, d: &mut FillPatternsDialog) {
    let Some(mut picked) = d.subject.as_ref().map(|(row, _)| *row) else { return };
    ui.horizontal(|ui| {
        ui.label(t("Type:"));
        let c = ComboBox::from_id_salt("region-edit-type")
            .selected_text(d.types.get(picked).map_or("", |r| r.name.as_str()))
            .width(230.0)
            .show_ui(ui, |ui| {
                for (i, r) in d.types.iter().enumerate() {
                    ui.selectable_value(&mut picked, i, &r.name);
                }
            });
        c.response.widget_info(|| WidgetInfo::labeled(WidgetType::ComboBox, true, t("Region Type")));
        if ui.button(t("Duplicate")).clicked()
            && let Some(r) = d.types.get(picked)
        {
            let mut copy = r.clone();
            copy.was = None;
            copy.name = fresh(&r.name, &d.types.iter().map(|r| r.name.as_str()).collect::<Vec<_>>());
            picked = d.types.len();
            d.types.push(copy);
        }
        if ui.button(t("New Type")).clicked() {
            picked = d.types.len();
            d.types.push(d.new_type());
        }
    });
    if let Some((row, _)) = &mut d.subject {
        *row = picked;
    }
    let names: Vec<(usize, String)> = d.live().map(|(i, p)| (i, p.name.clone())).collect();
    let Some(r) = d.types.get_mut(picked) else { return };
    ui.add_space(6.0);
    Grid::new("region-type-properties").num_columns(2).spacing(vec2(16.0, 8.0)).show(ui, |ui| {
        ui.label(t("Name"));
        name_field(ui, &mut r.name, t("Type Name").to_string());
        ui.end_row();
        for (row, rgb, label, colour) in [
            (&mut r.foreground, &mut r.foreground_color, msg!("Foreground Pattern"), msg!("Foreground Color")),
            (&mut r.background, &mut r.background_color, msg!("Background Pattern"), msg!("Background Color")),
        ] {
            ui.label(t(label));
            pattern_pick(ui, row, &names, t(label).to_string());
            ui.end_row();
            ui.label(t(colour));
            colour_field(ui, rgb, t(colour).to_string());
            ui.end_row();
        }
        ui.label("");
        ui.checkbox(&mut r.masking, t("Masking"));
        ui.end_row();
    });
    ui.add_space(8.0);
    ui.label(t("Preview"));
    if let Some(r) = d.types.get(picked) {
        preview(ui, r, &d.patterns);
    }
    ui.label(t("Changes to a type affect every filled region of that type."));
}

/// A type's sample as the plan draws a region of it, in the canvas's colours: a model line across the paper, which the
/// region's fills lie under (ink is kept above a region that doesn't mask) unless it masks, when the paper hides the line.
fn preview(ui: &mut Ui, r: &TypeRow, patterns: &[PatternRow]) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 100.0), Sense::hover());
    let (paper, ink) = (crate::canvas::color(PAPER), crate::canvas::color(Rgb(0, 0, 0)));
    let inner = rect.shrink(12.0);
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, paper);
    let line = || painter.line_segment([rect.left_center(), rect.right_center()], Stroke::new(2.0, ink));
    if r.masking {
        line();
        painter.rect_filled(inner, 0.0, paper);
    }
    for (row, rgb) in [(r.background, r.background_color), (r.foreground, r.foreground_color)] {
        let p = row.and_then(|i| patterns.get(i));
        paint_pattern(ui, inner, p.map(|p| &p.pattern), crate::canvas::color(Rgb(rgb[0], rgb[1], rgb[2])));
    }
    if !r.masking {
        line();
    }
    painter.rect_stroke(inner, 0.0, Stroke::new(1.0, canvas_pal().border), egui::StrokeKind::Inside);
}

/// The patterns: a row each, its name, drafting or model, a swatch, and (a simple one) its angle, spacing and crosshatch,
/// else how many line families it has; Edit Lines; Delete; New Pattern, Import and Export (what's asked of the app). The list
/// scrolls in what the window has left over `below` (the dialog's notes and buttons) and the row of buttons under it; when a
/// pattern's families are open, the list and they share it.
fn patterns_tab(ui: &mut Ui, d: &mut FillPatternsDialog, below: f32) -> Option<FileAsk> {
    ui.label(t("Families of parallel lines: drafting ones mm on paper, the same at any scale; model ones mm in the building."));
    ui.add_space(4.0);
    let ink = ui.visuals().text_color();
    let mut delete = None;
    let room = dialog_room(ui, below + dialog_row(ui));
    let editor = if d.editing.is_some() { room / 2.0 } else { 0.0 };
    ScrollArea::vertical().max_height(room - editor).show(ui, |ui| {
        Grid::new("fill-patterns").striped(true).num_columns(8).show(ui, |ui| {
            for h in ["Name", "Type", msg!("Sample"), "Angle", "Spacing", "Crosshatch", "", ""] {
                ui.label(RichText::new(t(h)).strong());
            }
            ui.end_row();
            for (i, r) in d.patterns.iter_mut().enumerate().filter(|(i, _)| !d.gone[*i]) {
                let who = r.name.clone();
                name_field(ui, &mut r.name, tf("{}: Name", &[&who]));
                let label = tf("{}: Type", &[&who]);
                let shown = t(if r.pattern.target == Target::Model { "Model" } else { "Drafting" });
                let c = ComboBox::from_id_salt(("fill-target", i)).selected_text(shown).width(90.0).show_ui(ui, |ui| {
                    ui.selectable_value(&mut r.pattern.target, Target::Drafting, t("Drafting"));
                    ui.selectable_value(&mut r.pattern.target, Target::Model, t("Model"));
                });
                c.response.widget_info(|| WidgetInfo::labeled(WidgetType::ComboBox, true, label.clone()));
                swatch(ui, Some(&r.pattern), 80.0, ink, None);
                match r.pattern.simple() {
                    Some(was) => {
                        let (mut angle, mut spacing, mut cross) = was;
                        let a = ui.add(DragValue::new(&mut angle).range(-360.0..=360.0).speed(1.0).suffix("°"));
                        a.widget_info(|| WidgetInfo::labeled(WidgetType::DragValue, true, tf("{}: Angle", &[&who])));
                        let s = ui.add(DragValue::new(&mut spacing).range(MIN_SPACING..=MAX_SPACING).speed(0.05).suffix(" mm"));
                        s.widget_info(|| WidgetInfo::labeled(WidgetType::DragValue, true, tf("{}: Spacing", &[&who])));
                        let c = ui.add(Checkbox::without_text(&mut cross));
                        c.widget_info(|| WidgetInfo::selected(WidgetType::Checkbox, true, cross, tf("{}: Crosshatch", &[&who])));
                        // Only a control touched rewrites the families: a crosshatch at 90° and 0° is a simple pattern
                        // too, but not the one `angle + 90` would make of it.
                        if (angle, spacing, cross) != was {
                            r.pattern.lines = vec![HatchLine::simple(angle, spacing)];
                            if cross {
                                r.pattern.lines.push(HatchLine::simple(angle + 90.0, spacing));
                            }
                        }
                    }
                    None if r.pattern.is_solid() => {
                        ui.label(t("Solid Fill"));
                        ui.label("");
                        ui.label("");
                    }
                    None => {
                        let families = r.pattern.lines.len().to_string();
                        ui.label(t("Custom")).on_hover_text(tf("{} line families", &[&families]));
                        ui.label("");
                        ui.label("");
                    }
                }
                if named_button(ui, t("Edit Lines"), tf("{}: Edit Lines", &[&who])) {
                    d.editing = Some((i, r.pattern.lines.iter().map(|l| dashes_text(&l.dashes)).collect()));
                }
                if named_button(ui, t("Delete"), tf("{}: Delete", &[&who])) {
                    delete = Some(i);
                }
                ui.end_row();
            }
        });
    });
    families(ui, d, editor);
    if let Some(i) = delete {
        d.gone[i] = true;
        if d.editing.as_ref().is_some_and(|(e, _)| *e == i) {
            d.editing = None;
        }
        for r in &mut d.types {
            for row in [&mut r.foreground, &mut r.background] {
                if *row == Some(i) {
                    *row = None;
                }
            }
        }
    }
    let mut ask = None;
    ui.horizontal(|ui| {
        if ui.button(t("New Pattern")).clicked() {
            let taken: Vec<&str> = d.live().map(|(_, p)| p.name.as_str()).collect();
            let name = fresh(t("Pattern"), &taken);
            let pattern = FillPattern { target: Target::Drafting, lines: vec![HatchLine::simple(45.0, 2.0)] };
            d.patterns.push(PatternRow { was: None, name, pattern });
            d.gone.push(false);
        }
        if ui.button(t("Import...")).on_hover_text(t("Patterns from a PAT file, AutoCAD's and Revit's")).clicked() {
            ask = Some(FileAsk::Import);
        }
        if ui.button(t("Export...")).on_hover_text(t("These patterns to a PAT file, but the solid ones")).clicked() {
            ask = Some(FileAsk::Export);
        }
        ui.checkbox(&mut d.replace, t("Replace Same Name"))
            .on_hover_text(t("A pattern of the file replaces the one of its name in this list; else it is left out"));
    });
    ask
}

impl FillPatternsDialog {
    /// What the draft asks of the project, in an order each can be done in: patterns renamed (so a chain or a swap goes
    /// through, [`renames`]), patterns made and changed, types renamed, made and changed, then what's deleted, types before
    /// patterns. A pattern or a type deleted and made again under its name is changed in place.
    fn commands(&self, m: &Model) -> Vec<(&'static str, Value)> {
        let mut out: Vec<(&'static str, Value)> = Vec::new();
        let rows: Vec<Row> =
            (self.patterns.iter().zip(&self.gone)).map(|(r, g)| Row { was: r.was.as_deref(), name: &r.name, gone: *g }).collect();
        let patterns = renames::plan(&m.fill_patterns.keys().cloned().collect(), &rows);
        out.extend(patterns.renames.iter().map(|(from, to)| ("fill_pattern", json!({"name": from, "rename": to}))));
        for (_, r) in self.live() {
            if r.was.as_ref().and_then(|w| m.fill_patterns.get(w)) != Some(&r.pattern) {
                out.push(("fill_pattern", json!({"name": r.name, "target": r.pattern.target, "lines": r.pattern.lines})));
            }
        }
        // A pattern renamed renames it in the types already: compare what the type holds read through the renames.
        let renamed = |n: &String| self.patterns.iter().find(|r| r.was.as_ref() == Some(n)).map_or_else(|| n.clone(), |r| r.name.clone());
        let kept: BTreeSet<&str> = self.types.iter().filter_map(|r| r.was.as_deref()).collect();
        let mut rows: Vec<Row> = self.types.iter().map(|r| Row { was: r.was.as_deref(), name: &r.name, gone: false }).collect();
        rows.extend(m.region_types.keys().filter(|n| !kept.contains(n.as_str())).map(|n| Row { was: Some(n), name: n, gone: true }));
        let types = renames::plan(&m.region_types.keys().cloned().collect(), &rows);
        out.extend(types.renames.iter().map(|(from, to)| ("filled_region_type", json!({"name": from, "rename": to}))));
        for r in &self.types {
            let mine = self.region_type(r);
            let same = |x: &FilledRegionType| {
                FilledRegionType {
                    foreground: x.foreground.as_ref().map(renamed),
                    background: x.background.as_ref().map(renamed),
                    ..x.clone()
                } == mine
            };
            if !r.was.as_ref().and_then(|w| m.region_types.get(w)).is_some_and(same) {
                out.push((
                    "filled_region_type",
                    json!({
                        "name": r.name, "foreground": mine.foreground, "foreground_color": mine.foreground_color,
                        "background": mine.background, "background_color": mine.background_color, "masking": mine.masking,
                    }),
                ));
            }
        }
        out.extend(types.deletes.iter().map(|n| ("filled_region_type", json!({"name": n, "delete": true}))));
        out.extend(patterns.deletes.iter().map(|n| ("fill_pattern", json!({"name": n, "delete": true}))));
        out
    }
}

impl BimApp {
    /// The Fill Patterns dialog, while it's open.
    pub(crate) fn fill_patterns_window(&mut self, ctx: &Context) {
        let Some(mut d) = self.fill_patterns_dialog.take() else { return };
        let editing = d.subject.is_some();
        let (mut ok, mut apply, mut cancel, mut ask) = (false, false, false, None);
        let r = Modal::new(UiId::new("fill-patterns")).show(ctx, |ui| {
            // One width for both tabs and both ways in, so the dialog doesn't jump as they change.
            ui.set_width(860.0);
            ui.label(RichText::new(t(if editing { "Type Properties" } else { msg!("Fill Patterns") })).heading());
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                for (i, tab) in TABS.iter().enumerate() {
                    if ui.selectable_label(d.tab == i, t(tab)).clicked() {
                        d.tab = i;
                    }
                }
            });
            ui.separator();
            // Under the tab: its notes, then OK, Cancel and Apply. What the window has left over them is the tab's, so that on a
            // small one the buttons stay in reach and the rows scroll.
            let below = messages_height(&d) + 8.0 + dialog_row(ui);
            if d.tab == 1 {
                ask = patterns_tab(ui, &mut d, below);
            } else if editing {
                ScrollArea::vertical().id_salt("edit-type").max_height(dialog_room(ui, below)).show(ui, |ui| edit_type(ui, &mut d));
            } else {
                types_tab(ui, &mut d, below);
            }
            messages(ui, &d);
            ui.add_space(8.0);
            ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                apply = ui.button(t("Apply")).clicked();
                cancel = ui.button(t("Cancel")).clicked();
                ok = ui.button(t("OK")).clicked();
            });
        });
        let failed = (ok || apply) && !self.apply_fill_patterns(&mut d, apply);
        // The draft is read only for the file it is exported to.
        let drafted = matches!(ask, Some(FileAsk::Export)).then(|| d.drafted());
        if failed || !(ok || cancel || r.should_close()) {
            self.fill_patterns_dialog = Some(d);
        }
        match (ask, drafted) {
            (Some(FileAsk::Import), _) => {
                let dialog = rfd::AsyncFileDialog::new().add_filter(t("Fill patterns"), &["pat", "PAT"]);
                self.ask_file(ctx, dialog, false, |app, path| app.import_patterns(&path));
            }
            (Some(FileAsk::Export), Some(drafted)) => {
                let dialog = rfd::AsyncFileDialog::new().add_filter(t("Fill patterns"), &["pat"]).set_file_name("patterns.pat");
                self.ask_file(ctx, dialog, true, move |app, path| app.export_patterns(&path, &drafted));
            }
            _ => {}
        }
    }

    /// OK and Apply: the draft done as one undo step, all or nothing, or refused (a project that changed since it opened, a
    /// name blank or used twice, the core's reasons) with the dialog saying why. Whether it was done.
    fn apply_fill_patterns(&mut self, d: &mut FillPatternsDialog, apply: bool) -> bool {
        let editing = d.subject.is_some();
        d.error = None;
        // Normalize names before validation and assignment, as the commands do.
        d.types.iter_mut().for_each(|r| r.name = r.name.trim().to_string());
        d.patterns.iter_mut().for_each(|r| r.name = r.name.trim().to_string());
        let selected = d.subject.as_ref().and_then(|(i, ids)| d.types.get(*i).map(|r| (r.name.clone(), ids.clone())));
        let done = d.check(&self.session.model).and_then(|()| {
            let mut commands = d.commands(&self.session.model);
            if let Some((name, ids)) = &selected
                && !ids.is_empty()
            {
                commands.push(("modify", json!({"ids": ids, "type": name})));
            }
            if commands.is_empty() {
                return Ok(());
            }
            self.session.transaction(if editing { "Type Properties" } else { "Fill Patterns" }, |s: &mut Session| {
                commands.iter().try_for_each(|(c, p)| s.execute(c, p).map(|_| ()))
            })
        });
        if let Err(e) = done {
            d.error = Some(e.clone());
            self.status = Status { text: e, error: true };
            return false;
        }
        self.status = Status::default();
        // The sketch is a draft too: keep its type reference in step with applied renames.
        if let Some(sk) = &mut self.sketch {
            if let Some(row) = d.types.iter().find(|r| r.was.as_ref() == Some(&sk.region.0)) {
                sk.region.0 = row.name.clone();
            }
            if let Some((name, ids)) = &selected
                && ids.is_empty()
            {
                sk.region.0 = name.clone();
            }
        }
        if let Some((name, _)) = &selected {
            self.types.insert("region", name.clone());
        }
        if apply {
            let tab = d.tab;
            *d = match selected {
                Some((name, ids)) => FillPatternsDialog::edit(&self.session.model, &name, ids),
                None => FillPatternsDialog::new(&self.session.model),
            };
            d.tab = tab;
        }
        true
    }

    /// The patterns of the PAT file at `path`, into the Fill Patterns dialog's draft; what came and what didn't, said in it.
    pub(crate) fn import_patterns(&mut self, path: &Path) {
        let Some(d) = &mut self.fill_patterns_dialog else { return };
        d.note = Some(d.read(path).unwrap_or_else(|e| (e, true)));
    }

    /// `patterns` written to the PAT file at `path`, ending in `.pat`: a file by that name that the system's Save didn't ask
    /// about is not written over.
    pub(crate) fn export_patterns(&mut self, path: &Path, patterns: &[(String, FillPattern)]) {
        let named = path.extension().is_some_and(|e| e.eq_ignore_ascii_case("pat"));
        let target = if named { path.to_path_buf() } else { path.with_extension("pat") };
        let (text, error) = if !named && target.exists() {
            (tf("{} already exists: save under a name ending in .pat to replace it.", &[&target.display().to_string()]), true)
        } else {
            let text = bimcraft_core::pat::write(patterns.iter().map(|(n, p)| (n, p)));
            match bimcraft_core::autosave::atomic_write(&target, &text) {
                Ok(()) => (tf("Fill patterns exported to {}", &[&target.display().to_string()]), false),
                Err(e) => (e, true),
            }
        };
        match &mut self.fill_patterns_dialog {
            Some(d) => d.note = Some((text, error)),
            None => self.status = Status { text, error },
        }
    }
}

/// The tallest the note is, in a box that scrolls.
const NOTE_HEIGHT: f32 = 60.0;

/// The note, when the Patterns tab shows it: what an import or export said belongs to the patterns, not to the types.
fn note(d: &FillPatternsDialog) -> Option<&(String, bool)> {
    d.note.as_ref().filter(|_| d.tab == 1)
}

/// How tall what `messages` draws can be, with the gaps between.
fn messages_height(d: &FillPatternsDialog) -> f32 {
    let gap = 6.0;
    note(d).map_or(0.0, |_| NOTE_HEIGHT + gap) + d.error.as_ref().map_or(0.0, |_| ERROR_HEIGHT + gap)
}

/// What the dialog says in itself, under its tab: the last import or export (a long list of what was left out scrolls), and
/// what stopped Apply.
fn messages(ui: &mut Ui, d: &FillPatternsDialog) {
    if let Some((text, bad)) = note(d) {
        boxed_text(ui, "fill-patterns-note", if *bad { pal().error } else { ui.visuals().text_color() }, text, NOTE_HEIGHT);
    }
    if let Some(error) = &d.error {
        boxed_text(ui, "fill-patterns-error", pal().error, error, ERROR_HEIGHT);
    }
}
