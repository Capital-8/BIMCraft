//! Filter on the selection, by category and by a parameter they share, driven as a user drives it.

use super::*;

/// The box's four walls, the east and west ones of a thicker type, and a door in the south one: all selected.
fn house() -> (Harness<'static, BimApp>, Vec<Id>, Id) {
    let mut h = app(BOX);
    let s = &mut h.state_mut().session;
    let mut walls: Vec<Id> = s.model.elements.iter().filter(|(_, e)| e.category() == "wall").map(|(id, _)| *id).collect();
    walls.sort_unstable();
    s.execute("type", &json!({"category": "wall", "name": "Thick", "thickness": 300})).unwrap();
    s.execute("modify", &json!({"ids": [walls[1], walls[3]], "type": "Thick"})).unwrap();
    let door = s.execute("door", &json!({"wall": walls[0], "offset": 2000})).unwrap()["id"].as_u64().unwrap();
    h.state_mut().selection = walls.iter().copied().chain([door]).collect();
    h.run();
    (h, walls, door)
}

#[test]
fn filter_leaves_a_cleared_category_out_of_the_selection() {
    let (mut h, walls, _) = house();
    press(&mut h, "Filter");
    assert!(h.state().selection_filter.is_some(), "the Select panel's Filter opens it");
    h.get_by_label("Total selected items: 5");
    pick(&mut h, Role::CheckBox, "Doors");
    h.get_by_label("Total selected items: 4");
    press(&mut h, "OK");
    assert!(h.state().selection_filter.is_none());
    assert_eq!(h.state().selection, walls);
}

#[test]
fn filter_by_a_parameter_the_walls_share_keeps_those_of_a_value() {
    let (mut h, walls, _) = house();
    h.state_mut().selection = walls.clone();
    h.state_mut().open_filter();
    h.run();
    let d = h.state().selection_filter.as_ref().unwrap();
    assert!(d.fields.iter().any(|f| f == "Type") && d.fields.iter().any(|f| f == "Length"), "{:?}", d.fields);
    h.state_mut().filter_by("Type");
    h.run();
    pick(&mut h, Role::CheckBox, "Generic - 200mm");
    h.get_by_label("Total selected items: 2");
    press(&mut h, "OK");
    assert_eq!(h.state().selection, [walls[1], walls[3]]);
}

#[test]
fn check_none_then_cancel_keeps_the_selection() {
    let (mut h, walls, door) = house();
    h.state_mut().open_filter();
    h.run();
    press(&mut h, "Check None");
    h.get_by_label("Total selected items: 0");
    press(&mut h, "Cancel");
    assert!(h.state().selection_filter.is_none());
    assert_eq!(h.state().selection, walls.iter().copied().chain([door]).collect::<Vec<_>>());
}

#[test]
fn the_status_bar_funnel_opens_the_filter() {
    let (mut h, _, _) = house();
    h.state_mut().selection_filter = None;
    assert!(h.state().selection_filter.is_none());
    press(&mut h, "Filter Selection");
    assert!(h.state().selection_filter.is_some());
}
