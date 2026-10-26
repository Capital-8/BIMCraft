//! Detail groups, repeating details and the annotation parameters in Properties: what UG undoes in one step, what a spacing
//! that isn't one makes, which rows a detail line shows, and the fill patterns in the materials window.

use super::*;

fn grouped(h: &mut Harness<BimApp>, at: P2) {
    click(h, at);
    shortcut(h, "GP");
    key(h, Key::Escape);
}

#[test]
fn ug_takes_every_group_selected_apart_in_one_undo_step() {
    let mut h = app(&format!(
        "{BOX}\ndetail_line {{\"start\": [1000, 1000], \"end\": [3000, 1000]}}\ndetail_line {{\"start\": [5000, 3000], \"end\": [7000, 3000]}}"
    ));
    grouped(&mut h, P2(2000.0, 1000.0));
    grouped(&mut h, P2(6000.0, 3000.0));
    assert_eq!((ids(&h, "detailgroup").len(), ids(&h, "line").len()), (2, 0));
    click(&mut h, P2(2000.0, 1000.0));
    click_with(&mut h, P2(6000.0, 3000.0), Modifiers::COMMAND);
    assert_eq!(h.state().selection.len(), 2);
    let steps = h.state().session.history().len();
    shortcut(&mut h, "UG");
    assert_eq!((ids(&h, "detailgroup").len(), ids(&h, "line").len()), (0, 2));
    assert_eq!(h.state().session.history().len(), steps + 1, "one step for both");
    assert_eq!(h.state().selection.len(), 2, "their lines, selected");
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.run();
    assert_eq!((ids(&h, "detailgroup").len(), ids(&h, "line").len()), (2, 0), "one Ctrl+Z puts both back");
}

#[test]
fn a_spacing_that_is_not_a_length_makes_no_repeating_detail_and_says_so() {
    let mut h = app(&format!("{BOX}\ndetail_line {{\"start\": [1000, 1000], \"end\": [1000, 1200]}}"));
    grouped(&mut h, P2(1000.0, 1100.0));
    press(&mut h, "Annotate");
    press(&mut h, "Repeating Detail");
    type_into(&mut h, "Spacing:", "often");
    click(&mut h, P2(1000.0, 3000.0));
    click(&mut h, P2(3000.0, 3000.0));
    assert!(ids(&h, "repeatingdetail").is_empty());
    assert!(h.state().status.error && h.state().status.text.contains("often"), "{}", h.state().status.text);
}

fn parameters(h: &mut Harness<BimApp>, id: Id) {
    h.state_mut().selection = vec![id];
    h.run();
}

fn shown(h: &Harness<BimApp>, label: &str) -> bool {
    h.query_all_by_label(label).count() > 0
}

#[test]
fn a_detail_line_shows_the_rows_that_mean_something_for_it() {
    let mut h = app(&format!(
        "{BOX}\ndetail_line {{\"start\": [1000, 1000], \"end\": [3000, 1000]}}\ndetail_line {{\"start\": [1000, 2000], \"end\": [3000, 2000], \"arc\": 90}}\ndetail_line {{\"start\": [1000, 4000], \"end\": [3000, 4000], \"cloud\": 500, \"revision\": \"R1\"}}"
    ));
    let lines = ids(&h, "line");
    // A straight line: a style and a length, no centre mark or revision to switch.
    parameters(&mut h, lines[0]);
    assert!(shown(&h, "Line Style") && shown(&h, "Length"));
    assert!(!shown(&h, "Center Mark Visible") && !shown(&h, "Revision"), "nothing a straight line could use");
    // An arc has a centre to mark.
    parameters(&mut h, lines[1]);
    assert!(shown(&h, "Center Mark Visible") && !shown(&h, "Revision"));
    // A cloud has an arc length and a revision.
    parameters(&mut h, lines[2]);
    assert!(shown(&h, "Revision") && shown(&h, "Arc Length") && !shown(&h, "Center Mark Visible"));
    assert_eq!(field_text(&h, "Revision"), "R1");
}

#[test]
fn a_regions_pattern_angle_a_groups_rotation_and_a_repeats_spacing_show_at_their_defaults() {
    let mut h = app(&format!(
        "{BOX}\nfilled_region {{\"sketch\": [[[1000,1000],[3000,1000]],[[3000,1000],[3000,3000]],[[3000,3000],[1000,3000]],[[1000,3000],[1000,1000]]]}}\ndetail_line {{\"start\": [5000, 1000], \"end\": [5000, 1200]}}"
    ));
    let region = ids(&h, "region")[0];
    parameters(&mut h, region);
    assert_eq!(field_text(&h, "Pattern Angle (°)"), "0", "the file leaves a zero out, the row doesn't");
    click(&mut h, P2(5000.0, 1100.0));
    shortcut(&mut h, "GP");
    key(&mut h, Key::Escape);
    let group = ids(&h, "detailgroup")[0];
    parameters(&mut h, group);
    assert_eq!(field_text(&h, "Rotation (°)"), "0");
    h.state_mut()
        .session
        .execute("repeating_detail", &json!({"type": "Group 1", "start": [1000, 5000], "end": [3000, 5000], "spacing": 500}))
        .unwrap();
    let repeat = ids(&h, "repeatingdetail")[0];
    parameters(&mut h, repeat);
    assert_eq!(field_text(&h, "Spacing"), "500");
    assert!(shown(&h, "Number"));
}

#[test]
fn the_materials_window_offers_the_projects_fill_patterns_besides_its_own() {
    let mut h = app(BOX);
    press(&mut h, "Manage");
    press(&mut h, "Materials");
    press(&mut h, "New");
    choose(&mut h, "Cut Pattern", "Sand");
    let m = &h.state().session.model.materials["Material 1"];
    assert_eq!((m.cut_pattern.as_deref(), m.surface_pattern.as_deref()), (Some("Sand"), None));
    choose(&mut h, "Surface Pattern", "Tile 300");
    assert_eq!(h.state().session.model.materials["Material 1"].surface_pattern.as_deref(), Some("Tile 300"));
    // And the combo shows the one it has.
    assert_eq!(combo_value(&h, "Cut Pattern"), "Sand");
}
