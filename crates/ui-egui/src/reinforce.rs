//! Structure > Reinforce (RF): a dialog that arms the selected beam, column, footing or floor by the `reinforce` command. The
//! engineer types how many bars, how thick and how far apart; the dialog builds the command's parameters from the fields that are
//! filled and the command checks them, so a refusal is the command's words, said inside the dialog. What was typed for a kind of
//! host is kept for the next of that kind.

use std::collections::BTreeMap;

use bimcraft_i18n::msg;

use bimcraft_core::model::{Element, Id};
use bimcraft_core::units::Units;
use egui::{Align, Context, Id as UiId, Layout, Modal, RichText, ScrollArea, TextEdit, Ui, WidgetInfo, WidgetType};
use serde_json::{Value, json};

use crate::lang::{category, t, tf};
use crate::style::pal;
use crate::{BimApp, Status};

/// How a field's text is read: a whole number, a bar size in mm, or a length in the project's units.
#[derive(Clone, Copy)]
enum Kind {
    Count,
    Diameter,
    Length,
}

/// A field of the form: the path of the parameter it sets (`bottom.count`), its label and how it reads.
struct Field {
    path: &'static str,
    label: &'static str,
    kind: Kind,
}

/// A row of the form: a name and its fields; a row with no field filled is left out of the parameters.
struct Group {
    name: &'static str,
    fields: &'static [Field],
}

const fn field(path: &'static str, label: &'static str, kind: Kind) -> Field {
    Field { path, label, kind }
}

const BOTTOM: Group = Group {
    name: msg!("Bottom bars"),
    fields: &[field("bottom.count", msg!("Count"), Kind::Count), field("bottom.diameter", msg!("Diameter"), Kind::Diameter)],
};
const TOP: Group = Group {
    name: msg!("Top bars"),
    fields: &[field("top.count", msg!("Count"), Kind::Count), field("top.diameter", msg!("Diameter"), Kind::Diameter)],
};
const STIRRUPS: Group = Group {
    name: msg!("Stirrups"),
    fields: &[
        field("stirrup.diameter", msg!("Diameter"), Kind::Diameter),
        field("stirrup.spacing", msg!("Spacing"), Kind::Length),
        field("stirrup.ends.length", msg!("End zone length"), Kind::Length),
        field("stirrup.ends.spacing", msg!("End zone spacing"), Kind::Length),
        field("stirrup.hook", msg!("Hook"), Kind::Length),
    ],
};

/// The forms, one for each kind of host.
const BEAM: &[Group] = &[
    BOTTOM,
    TOP,
    STIRRUPS,
    Group {
        name: msg!("Skin bars"),
        fields: &[field("skin.diameter", msg!("Diameter"), Kind::Diameter), field("skin.spacing", msg!("Spacing"), Kind::Length)],
    },
    Group {
        name: msg!("Anchor"),
        fields: &[field("anchor.start", msg!("Start"), Kind::Length), field("anchor.end", msg!("End"), Kind::Length)],
    },
];
const COLUMN: &[Group] = &[
    Group {
        name: msg!("Bars"),
        fields: &[
            field("bars.diameter", msg!("Diameter"), Kind::Diameter),
            field("bars.x", msg!("Along width"), Kind::Count),
            field("bars.y", msg!("Along depth"), Kind::Count),
        ],
    },
    STIRRUPS,
    Group { name: msg!("Ties"), fields: &[field("ties.diameter", msg!("Diameter"), Kind::Diameter)] },
    Group {
        name: msg!("Anchor"),
        fields: &[field("anchor.below", msg!("Below"), Kind::Length), field("anchor.above", msg!("Above"), Kind::Length)],
    },
];
const FOOTING: &[Group] = &[
    Group {
        name: msg!("Mesh along width"),
        fields: &[field("mesh.x.diameter", msg!("Diameter"), Kind::Diameter), field("mesh.x.spacing", msg!("Spacing"), Kind::Length)],
    },
    Group {
        name: msg!("Mesh along depth"),
        fields: &[field("mesh.y.diameter", msg!("Diameter"), Kind::Diameter), field("mesh.y.spacing", msg!("Spacing"), Kind::Length)],
    },
    Group { name: msg!("Ends turned up"), fields: &[field("mesh.hook", msg!("Hook"), Kind::Length)] },
];
const FLOOR: &[Group] = &[
    Group {
        name: msg!("Bottom, bars along east"),
        fields: &[field("bottom.x.diameter", msg!("Diameter"), Kind::Diameter), field("bottom.x.spacing", msg!("Spacing"), Kind::Length)],
    },
    Group {
        name: msg!("Bottom, bars along north"),
        fields: &[field("bottom.y.diameter", msg!("Diameter"), Kind::Diameter), field("bottom.y.spacing", msg!("Spacing"), Kind::Length)],
    },
    Group {
        name: msg!("Top, bars along east"),
        fields: &[field("top.x.diameter", msg!("Diameter"), Kind::Diameter), field("top.x.spacing", msg!("Spacing"), Kind::Length)],
    },
    Group {
        name: msg!("Top, bars along north"),
        fields: &[field("top.y.diameter", msg!("Diameter"), Kind::Diameter), field("top.y.spacing", msg!("Spacing"), Kind::Length)],
    },
];

/// The cover's fields, and the steel's and the room between bars (blank is the command's usual).
const COVER: [(&str, &str); 4] =
    [("cover", msg!("Cover")), ("cover_top", msg!("Top")), ("cover_bottom", msg!("Bottom")), ("cover_sides", msg!("Sides"))];

/// What a host is armed as: the form it has.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Form {
    Beam,
    Column,
    Footing,
    Floor,
}

impl Form {
    fn of(e: &Element) -> Option<Form> {
        match e {
            Element::Beam(_) => Some(Form::Beam),
            Element::Column(_) | Element::StructuralColumn(_) => Some(Form::Column),
            Element::Footing(_) => Some(Form::Footing),
            Element::Floor(_) => Some(Form::Floor),
            _ => None,
        }
    }

    fn groups(self) -> &'static [Group] {
        match self {
            Form::Beam => BEAM,
            Form::Column => COLUMN,
            Form::Footing => FOOTING,
            Form::Floor => FLOOR,
        }
    }
}

/// The Reinforce dialog: the host, what is typed (by the parameter each field sets), what the command last refused, and the bars
/// the command made for the host before, which Remove Bars takes away.
pub(crate) struct ReinforceDialog {
    host: Id,
    category: &'static str,
    form: Form,
    values: BTreeMap<&'static str, String>,
    pub(crate) error: String,
    made: Vec<Id>,
}

/// `value` at the dotted `path` of `p`, the objects on the way made.
fn set(p: &mut Value, path: &str, value: Value) {
    let mut at = p;
    for key in path.split('.') {
        let Some(map) = at.as_object_mut() else { return };
        at = map.entry(key.to_string()).or_insert(Value::Null);
        if at.is_null() {
            *at = json!({});
        }
    }
    *at = value;
}

impl ReinforceDialog {
    /// The text of field `key`, trimmed.
    fn text(&self, key: &str) -> &str {
        self.values.get(key).map_or("", |s| s.trim())
    }

    /// What field `key`, called `name`, says, if it says anything.
    fn read(&self, units: Units, key: &str, kind: Kind, name: &str) -> Result<Option<Value>, String> {
        let text = self.text(key);
        if text.is_empty() {
            return Ok(None);
        }
        let not = |what: &str| tf("{} is not {}: `{}`", &[name, what, text]);
        match kind {
            Kind::Count => text.parse::<u32>().map(|n| Some(json!(n))).map_err(|_| not(t("a whole number"))),
            Kind::Diameter => text
                .replace(',', ".")
                .parse::<f64>()
                .ok()
                .filter(|x| x.is_finite())
                .map(|x| Some(json!(x)))
                .ok_or_else(|| not(t("a size in mm"))),
            Kind::Length => units.parse(text).map(|x| Some(json!(x))).ok_or_else(|| not(t("a length"))),
        }
    }

    /// The command's parameters from the fields filled.
    fn params(&self, units: Units) -> Result<Value, String> {
        let mut p = json!({"host": self.host});
        for g in self.form.groups() {
            for f in g.fields {
                let name = format!("{}: {}", t(g.name), t(f.label));
                if let Some(v) = self.read(units, f.path, f.kind, &name)? {
                    set(&mut p, f.path, v);
                }
            }
        }
        let face = |key: &str, label: &str| self.read(units, key, Kind::Length, &format!("{}: {}", t("Cover"), t(label)));
        let (all, top, bottom, sides) = (
            self.read(units, "cover", Kind::Length, t("Cover"))?,
            face("cover_top", "Top")?,
            face("cover_bottom", "Bottom")?,
            face("cover_sides", "Sides")?,
        );
        if top.is_some() || bottom.is_some() || sides.is_some() {
            let mut cover = json!({});
            for (key, v) in [("top", top), ("bottom", bottom), ("sides", sides)] {
                if let Some(v) = v.or_else(|| all.clone()) {
                    cover[key] = v;
                }
            }
            p["cover"] = cover;
        } else if let Some(all) = all {
            p["cover"] = all;
        }
        for (key, param) in [("grade", "grade"), ("grade_stirrup", "grade_stirrup")] {
            if !self.text(key).is_empty() {
                p[param] = json!(self.text(key));
            }
        }
        if let Some(v) = self.read(units, "clearance", Kind::Length, t("Steel: Clearance"))? {
            p["clearance"] = v;
        }
        Ok(p)
    }
}

/// A text field named `name` for screen readers, `width` wide.
fn edit(ui: &mut Ui, text: &mut String, name: String, width: f32, hint: &str) {
    let r = ui.add(TextEdit::singleline(text).desired_width(width).hint_text(hint));
    r.widget_info(|| WidgetInfo::labeled(WidgetType::TextEdit, true, name.clone()));
}

impl BimApp {
    /// Reinforce (RF): the dialog for the element selected, if it is one that can be armed.
    pub(crate) fn open_reinforce(&mut self) {
        let m = &self.session.model;
        let host = match self.selection.as_slice() {
            [id] => m.elements.get(*id).and_then(|e| Form::of(e).map(|f| (*id, e.category(), f))),
            _ => None,
        };
        let Some((id, category, form)) = host else {
            self.status = Status { text: t("Select a beam, a column, a footing or a floor to reinforce first.").into(), error: true };
            return;
        };
        let guid = m.guid(id);
        let made = m
            .elements
            .iter()
            .filter(|(_, e)| matches!(e, Element::Rebar(r) if r.host == Some(guid) && !r.layer.is_empty()))
            .map(|(i, _)| *i)
            .collect();
        let values = self.reinforce_kept.get(&form).cloned().unwrap_or_default();
        self.reinforce_dialog = Some(ReinforceDialog { host: id, category, form, values, error: String::new(), made });
    }

    /// The Reinforce dialog, while it is open.
    pub(crate) fn reinforce_window(&mut self, ctx: &Context) {
        let Some(mut d) = self.reinforce_dialog.take() else { return };
        let units = self.session.model.units;
        let hint = units.name();
        let (mut ok, mut cancel, mut remove) = (false, false, false);
        let r = Modal::new(UiId::new("reinforce")).show(ctx, |ui| {
            ui.set_width(700.0);
            ui.label(RichText::new(tf("Reinforce {} {}", &[&category(d.category, false), &d.host.to_string()])).heading());
            ui.add_space(4.0);
            ui.label(t("How many bars, how thick and how far apart is yours to say; a field left empty is not used. A spacing is the most between bars."));
            ui.separator();
            ScrollArea::vertical().max_height(480.0).show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    for (key, label) in COVER {
                        // The first field is the cover, the others its faces; its label has a colon so no two nodes carry its name.
                        let name = if key == "cover" { t(label).to_string() } else { format!("{}: {}", t("Cover"), t(label)) };
                        ui.horizontal(|ui| {
                            ui.label(if key == "cover" { tf("{}:", &[t(label)]) } else { t(label).to_string() });
                            edit(ui, d.values.entry(key).or_default(), name, 56.0, hint);
                        });
                    }
                });
                for g in d.form.groups() {
                    ui.add_space(4.0);
                    ui.horizontal_wrapped(|ui| {
                        ui.label(RichText::new(t(g.name)).strong());
                        for f in g.fields {
                            ui.horizontal(|ui| {
                                ui.label(t(f.label));
                                let h = if matches!(f.kind, Kind::Length) { hint } else { "" };
                                edit(ui, d.values.entry(f.path).or_default(), format!("{}: {}", t(g.name), t(f.label)), 56.0, h);
                            });
                        }
                    });
                }
                ui.add_space(4.0);
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new(t("Steel")).strong());
                    ui.horizontal(|ui| {
                        ui.label(t("Grade"));
                        edit(ui, d.values.entry("grade").or_default(), t("Steel: Grade").to_string(), 64.0, "CA-50");
                    });
                    ui.horizontal(|ui| {
                        ui.label(t("Stirrup grade"));
                        edit(ui, d.values.entry("grade_stirrup").or_default(), t("Steel: Stirrup grade").to_string(), 64.0, "");
                    });
                    ui.horizontal(|ui| {
                        ui.label(t("Clearance"));
                        edit(ui, d.values.entry("clearance").or_default(), t("Steel: Clearance").to_string(), 56.0, hint);
                    });
                });
            });
            if !d.error.is_empty() {
                ui.add_space(6.0);
                ui.label(RichText::new(&d.error).color(pal().error));
            }
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if !d.made.is_empty() {
                    remove = ui.button(t("Remove Bars")).on_hover_text(t("Take away the bars this command made for the element: bars drawn by hand stay")).clicked();
                }
                ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                    cancel = ui.button(t("Cancel")).clicked();
                    ok = ui.button(t("OK")).clicked();
                });
            });
        });
        let mut keep = !(ok || cancel || remove || r.should_close());
        if ok {
            match d.params(units) {
                Err(e) => {
                    d.error = e;
                    keep = true;
                }
                Ok(p) => match self.run("reinforce", p) {
                    Some(reply) => {
                        let (count, kg) = (reply["count"].as_u64().unwrap_or(0), reply["weight_kg"].as_f64().unwrap_or(0.0));
                        self.status = Status {
                            text: tf("Reinforced: {} bars, {} kg of steel.", &[&count.to_string(), &kg.to_string()]),
                            error: false,
                        };
                    }
                    None => {
                        // The command's words, in the dialog: a status box in the view's corner can lie over its buttons.
                        d.error = std::mem::take(&mut self.status.text);
                        self.status = Status::default();
                        keep = true;
                    }
                },
            }
        }
        if remove && self.run("delete", json!({"ids": d.made})).is_some() {
            self.status = Status { text: tf("Removed {} bars.", &[&d.made.len().to_string()]), error: false };
        }
        if keep {
            self.reinforce_dialog = Some(d);
        } else {
            // What was typed stays for the next host of its kind.
            self.reinforce_kept.insert(d.form, d.values);
        }
    }
}
