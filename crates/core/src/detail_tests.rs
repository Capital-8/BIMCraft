//! Detailing around the detail lines: repeating details and groups, Linework, the files that hold them, and the hostile
//! inputs each cap is for, driven through commands as a front end drives them.

use serde_json::{Value, json};

use crate::curves::Curve;
use crate::engine::Session;
use crate::geom::P2;
use crate::groups::{GroupType, Member, RepeatingDetail};
use crate::ids::Guid;
use crate::lines::DetailLine;
use crate::model::{Element, Id, Model};

fn run(s: &mut Session, c: &str, p: Value) -> Value {
    s.execute(c, &p).unwrap_or_else(|e| panic!("{c} {p}: {e}"))
}

fn err(s: &mut Session, c: &str, p: Value) -> String {
    s.execute(c, &p).err().unwrap_or_else(|| panic!("{c} {p} was accepted"))
}

fn id_of(v: &Value) -> Id {
    v["id"].as_u64().unwrap_or_else(|| panic!("no id in {v}"))
}

/// A box of walls on Level 1, and a group type `G` of `members` short detail lines, put straight into the model (making
/// 10,000 through commands would take a check each).
fn with_group(members: usize) -> Session {
    let mut s = Session::new();
    run(&mut s, "wall", json!({"points": [[0, 0], [8000, 0], [8000, 6000], [0, 6000]], "closed": true}));
    let line = |i: usize| Member {
        id: 0,
        guid: Guid::default(),
        element: Element::DetailLine(DetailLine {
            view: Guid::default(),
            curve: Curve::line(P2(i as f64, 0.0), P2(i as f64, 5.0)),
            style: "Thin Lines".into(),
            insulation: None,
            centre_mark: false,
            cloud: None,
            revision: None,
        }),
    };
    s.model_mut().detail_groups.insert("G".into(), GroupType { members: (0..members).map(line).collect() });
    s.model_mut().sync_library();
    s
}

/// A repeating detail of `G` in Level 1's plan: `count` of them, `spacing` apart.
fn repeat(count: usize, spacing: f64) -> Value {
    json!({"type": "G", "start": [0, 0], "end": [(count - 1) as f64 * spacing, 0], "spacing": spacing, "view": 100000})
}

#[test]
fn a_repeating_detail_may_place_as_many_members_as_a_view_draws_and_no_more() {
    let max = crate::groups::MAX_PLACED;
    // 100 members to the repeat: `max / 100` repeats are the most.
    let mut s = with_group(100);
    let fit = max / 100;
    assert_eq!(run(&mut s, "repeating_detail", repeat(fit, 10.0))["count"], fit);
    let mut s = with_group(100);
    let e = err(&mut s, "repeating_detail", repeat(fit + 1, 10.0));
    assert!(e.contains("members") && e.contains(&max.to_string()), "{e}");
    assert_eq!(s.model.elements.values().filter(|e| matches!(e, Element::RepeatingDetail(_))).count(), 0, "rolled back");
    // The old hostile case: 2000 repeats of a hundred members.
    let e = err(&mut s, "repeating_detail", repeat(2000, 10.0));
    assert!(e.contains("members"), "{e}");
    // A group type of the most members, repeated: 10,000 x 2 is over.
    let mut s = with_group(crate::groups::MAX_MEMBERS);
    run(&mut s, "repeating_detail", repeat(1, 10.0));
    assert!(err(&mut s, "repeating_detail", repeat(2, 10.0)).contains("members"));
}

#[test]
fn what_a_view_places_is_counted_over_all_its_groups_and_repeats_and_not_over_other_views() {
    let max = crate::groups::MAX_PLACED;
    let mut s = with_group(100);
    run(&mut s, "repeating_detail", repeat(max / 100, 10.0));
    // The view is full: one more group of 100 members in it is over, and in the other level's plan it isn't.
    let e = err(&mut s, "detail_group", json!({"type": "G", "at": [0, 0], "view": 100000}));
    assert!(e.contains("members"), "{e}");
    run(&mut s, "detail_group", json!({"type": "G", "at": [0, 0], "view": 100001}));
    // A repeat made fewer, then more again by `modify`: the rule holds for edits too.
    let mut s = with_group(100);
    let id = id_of(&run(&mut s, "repeating_detail", repeat(10, 100.0)));
    let e = err(&mut s, "modify", json!({"id": id, "spacing": 1.0, "end": [9000, 0]}));
    assert!(e.contains("members") || e.contains("repeat"), "{e}");
    // A file that holds more is refused whole, as the model's check says.
    let mut s = with_group(100);
    let view = s.model.guid(100000);
    let at = RepeatingDetail { view, type_name: "G".into(), start: P2(0.0, 0.0), end: P2(10_000.0, 0.0), spacing: 10.0, mirrored: false };
    s.model_mut().insert(Element::RepeatingDetail(at));
    assert!(s.model.check().is_err_and(|e| e.contains("members")));
}

/// Level 1's plan with a box of walls, and the box's first wall's GUID.
fn walled() -> (Session, Id, Guid) {
    let mut s = Session::new();
    let w = run(&mut s, "wall", json!({"points": [[0, 0], [8000, 0], [8000, 6000], [0, 6000]], "closed": true}))["ids"][0]
        .as_u64()
        .unwrap_or_default();
    let g = s.model.guid(w);
    (s, w, g)
}

#[test]
fn a_views_linework_is_counted_in_range_and_in_a_line_style_there_is() {
    let (mut s, _, wall) = walled();
    let edge = |i: usize| json!({"element": wall, "from": [i as f64, 0.0], "to": [i as f64 + 1.0, 0.0], "style": "Wide Lines"});
    run(&mut s, "modify", json!({"id": 100000, "graphics": {"linework": [edge(0)]}}));
    let bad = [
        json!({"element": wall, "from": [1e300, 0.0], "to": [0.0, 0.0], "style": "Wide Lines"}),
        json!({"element": wall, "from": [0.0, 0.0], "to": [0.0, -2e9], "style": "Wide Lines"}),
        json!({"element": wall, "from": [0.0, 0.0], "to": [1.0, 0.0], "style": "No Such Style"}),
    ];
    for b in bad {
        let e = err(&mut s, "modify", json!({"id": 100000, "graphics": {"linework": [b]}}));
        assert!(e.contains("linework") || e.contains("Linework"), "{e}");
    }
    // The most, and one over it.
    let most = crate::graphics::MAX_LINEWORK;
    run(&mut s, "modify", json!({"id": 100000, "graphics": {"linework": (0..most).map(edge).collect::<Vec<_>>()}}));
    let e = err(&mut s, "modify", json!({"id": 100000, "graphics": {"linework": (0..=most).map(edge).collect::<Vec<_>>()}}));
    assert!(e.contains(&most.to_string()), "{e}");
}

#[test]
fn linework_remove_takes_off_the_nearest_edge_and_not_every_edge_within_reach() {
    let mut s = Session::new();
    run(&mut s, "type", json!({"category": "wall", "name": "Partition", "thickness": 100}));
    let w = run(&mut s, "wall", json!({"points": [[0, 0], [4000, 0]], "type": "Partition"}))["ids"][0].as_u64().unwrap_or_default();
    let held = |s: &Session| crate::graphics::graphics_of(&s.model, 100000).map_or(0, |g| g.linework.len());
    // Each face of the 100 mm wall, restyled.
    run(&mut s, "linework", json!({"view": 100000, "id": w, "at": [1000, 45], "style": "Wide Lines"}));
    run(&mut s, "linework", json!({"view": 100000, "id": w, "at": [1000, -45], "invisible": true}));
    assert_eq!(held(&s), 2);
    let r = run(&mut s, "linework", json!({"view": 100000, "id": w, "at": [1000, 45], "remove": true}));
    assert_eq!((r["removed"].clone(), held(&s)), (json!(1), 1), "{r}");
    let r = run(&mut s, "linework", json!({"view": 100000, "id": w, "at": [1000, -45], "remove": true}));
    assert_eq!((r["removed"].clone(), held(&s)), (json!(1), 0), "{r}");
    // Nothing near: nothing removed.
    let r = run(&mut s, "linework", json!({"view": 100000, "id": w, "at": [1000, 45], "remove": true}));
    assert_eq!(r["removed"], 0);
}

#[test]
fn an_unknown_field_in_a_group_members_element_is_left_out_as_it_is_in_any_elements() {
    let mut s = Session::new();
    run(&mut s, "wall", json!({"points": [[0, 0], [4000, 0]]}));
    let a = id_of(&run(&mut s, "detail_line", json!({"start": [0, 0], "end": [1000, 0]})));
    let b = id_of(&run(&mut s, "detail_line", json!({"start": [0, 500], "end": [1000, 500]})));
    let c = id_of(&run(&mut s, "detail_line", json!({"start": [0, 900], "end": [1000, 900]})));
    run(&mut s, "create_group", json!({"ids": [a, b], "name": "A/B~C"}));
    let mut file: Value = serde_json::from_str(&s.model.to_json().unwrap_or_default()).unwrap_or_default();
    let key = file["detail_groups"].as_object().and_then(|o| o.keys().next().cloned()).unwrap_or_default();
    file["detail_groups"][&key]["members"][1]["element"]["made_later"] = json!(1);
    file["elements"][c.to_string()]["made_later"] = json!(2);
    let (m, left_out) = Model::read(&file.to_string()).unwrap_or_else(|e| panic!("{e}"));
    let at = format!("/detail_groups/{}/members/1/element/made_later", key.replace('~', "~0").replace('/', "~1"));
    assert!(left_out.contains(&at), "{left_out:?}");
    assert!(left_out.contains(&format!("/elements/{c}/made_later")), "{left_out:?}");
    assert_eq!(m.detail_groups.values().map(|g| g.members.len()).collect::<Vec<_>>(), [2]);
    // A newer field in a member of a member's sketch (a region's line) too.
    let r =
        id_of(&run(&mut s, "filled_region", json!({"sketch": [[[0, 0], [1000, 0]], [[1000, 0], [1000, 1000]], [[1000, 1000], [0, 0]]]})));
    run(&mut s, "create_group", json!({"ids": [r], "name": "R"}));
    let mut file: Value = serde_json::from_str(&s.model.to_json().unwrap_or_default()).unwrap_or_default();
    let key = file["detail_groups"].as_object().and_then(|o| o.keys().find(|k| *k != &key).cloned()).unwrap_or_default();
    file["detail_groups"][&key]["members"][0]["element"]["sketch"][0]["made_later"] = json!(1);
    let (_, left_out) = Model::read(&file.to_string()).unwrap_or_else(|e| panic!("{e}"));
    assert!(left_out.iter().any(|p| p.ends_with("/sketch/0/made_later")), "{left_out:?}");
}

#[test]
fn a_line_style_a_group_member_or_a_region_uses_is_kept_and_the_error_says_which() {
    let mut s = Session::new();
    run(&mut s, "wall", json!({"points": [[0, 0], [4000, 0]]}));
    run(&mut s, "line_style", json!({"name": "Thick", "pen": 4}));
    run(&mut s, "line_style", json!({"name": "Edge", "pen": 2}));
    let a = id_of(&run(&mut s, "detail_line", json!({"start": [0, 0], "end": [1000, 0], "style": "Thick"})));
    run(&mut s, "create_group", json!({"ids": [a], "name": "Section Mark"}));
    let e = err(&mut s, "line_style", json!({"name": "Thick", "delete": true}));
    assert!(e.contains("detail group `Section Mark` has a member drawn in `Thick`"), "{e}");
    // A region of the project, bounded in a style.
    let sq = json!([[[0, 0], [1000, 0]], [[1000, 0], [1000, 1000]], [[1000, 1000], [0, 1000]], [[0, 1000], [0, 0]]]);
    let r = id_of(&run(&mut s, "filled_region", json!({"sketch": sq, "line_style": "Edge"})));
    let e = err(&mut s, "line_style", json!({"name": "Edge", "delete": true}));
    assert!(e.contains(&format!("filled region {r} is bounded in `Edge`")), "{e}");
    // In a group, the region's style is named with the group too.
    run(&mut s, "create_group", json!({"ids": [r], "name": "Boxes"}));
    let e = err(&mut s, "line_style", json!({"name": "Edge", "delete": true}));
    assert!(e.contains("detail group `Boxes` has a member drawn in `Edge`"), "{e}");
    // A style nothing uses is deleted as ever.
    run(&mut s, "line_style", json!({"name": "Overhead Lines", "delete": true}));
}

#[test]
fn a_mirrored_group_turns_its_notes_with_it_and_keeps_them_readable() {
    let mut s = Session::new();
    run(&mut s, "wall", json!({"points": [[0, 0], [4000, 0]]}));
    let note = id_of(&run(&mut s, "text", json!({"text": "Fix", "at": [0, 0], "angle": 30})));
    run(&mut s, "create_group", json!({"ids": [note], "name": "Note"}));
    let angle = |s: &Session, turn: f64, mirrored: bool| {
        let g =
            crate::groups::DetailGroup { view: s.model.guid(100000), type_name: "Note".into(), at: P2(0.0, 0.0), angle: turn, mirrored };
        match crate::groups::placed(&s.model, &g).first() {
            Some(Element::Text(n)) => n.angle,
            e => panic!("{e:?}"),
        }
    };
    assert_eq!(angle(&s, 0.0, false), 30.0);
    assert_eq!(angle(&s, 40.0, false), 70.0, "turned with the group");
    assert_eq!(angle(&s, 0.0, true), 30.0, "a pure mirror leaves a note reading the way it did");
    assert_eq!(angle(&s, 40.0, true), 70.0, "mirrored and turned, it turns too");
}

#[test]
fn a_regions_or_floors_pattern_origin_and_angle_show_in_elements_and_align_moves_the_origin() {
    let mut s = Session::new();
    run(&mut s, "wall", json!({"points": [[0, 0], [4000, 0], [4000, 4000], [0, 4000]], "closed": true}));
    run(&mut s, "filled_region_type", json!({"name": "Tiles", "foreground": "Tile 300"}));
    let sq = json!([[[0, 0], [3000, 0]], [[3000, 0], [3000, 3000]], [[3000, 3000], [0, 3000]], [[0, 3000], [0, 0]]]);
    let region = id_of(&run(&mut s, "filled_region", json!({"sketch": sq, "type": "Tiles", "line_style": null})));
    let floor = id_of(&run(&mut s, "floor", json!({"boundary": [[0, 0], [4000, 0], [4000, 4000], [0, 4000]]})));
    for id in [region, floor] {
        let e = crate::element_json(&s.model, id);
        assert_eq!((e["pattern_origin"].clone(), e["pattern_angle"].clone()), (json!([0.0, 0.0]), json!(0.0)), "{e}");
    }
    // The tile line nearest x = 630 lies on x = 600; a reference at x = 650 asks for the origin 50 mm east.
    let near = |s: &Session| crate::render::pattern_line_at(&s.model, 100000, region, P2(640.0, 1650.0), 100.0);
    let line = near(&s).unwrap_or_else(|| panic!("no pattern line there"));
    assert!((line.0.0 - 600.0).abs() < 0.5 && (line.1.0 - 600.0).abs() < 0.5, "{line:?}");
    let reference = (P2(650.0, 0.0), P2(650.0, 3000.0));
    let to = s.model.align_pattern(region, reference, line).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(to, P2(50.0, 0.0));
    run(&mut s, "modify", json!({"id": region, "pattern_origin": [to.0, to.1]}));
    let after = near(&s).unwrap_or_else(|| panic!("no pattern line there"));
    assert!((after.0.0 - 650.0).abs() < 0.5, "{after:?}");
    // Aligned again from there it stays; a floor takes it the same way; and what can't be aligned says why.
    assert_eq!(s.model.align_pattern(region, reference, after), Ok(to));
    assert_eq!(s.model.align_pattern(floor, reference, line), Ok(P2(50.0, 0.0)));
    let e = s.model.align_pattern(region, (P2(0.0, 0.0), P2(1000.0, 0.0)), line).unwrap_err();
    assert!(e.contains("parallel"), "{e}");
    let e = s.model.align_pattern(region, (P2(5.0, 5.0), P2(5.0, 5.0)), line).unwrap_err();
    assert!(e.contains("no length"), "{e}");
    let wall = s.model.elements.iter().find(|(_, e)| matches!(e, Element::Wall(_))).map(|(i, _)| *i).unwrap_or_default();
    assert!(s.model.align_pattern(wall, reference, line).unwrap_err().contains("pattern"));
    assert!(s.model.align_pattern(u64::MAX, reference, line).is_err());
    // A turned pattern aligns across its own lines' way: a reference at 45 degrees, the lines turned to match.
    run(&mut s, "modify", json!({"id": region, "pattern_origin": [0, 0], "pattern_angle": 45}));
    let diag = crate::render::pattern_line_at(&s.model, 100000, region, P2(1500.0, 1560.0), 100.0).unwrap_or_else(|| panic!("no line"));
    let r45 = (P2(0.0, 100.0), P2(1000.0, 1100.0));
    let moved = s.model.align_pattern(region, r45, diag).unwrap_or_else(|e| panic!("{e}"));
    run(&mut s, "modify", json!({"id": region, "pattern_origin": [moved.0, moved.1]}));
    let again = crate::render::pattern_line_at(&s.model, 100000, region, P2(1500.0, 1640.0), 100.0).unwrap_or_else(|| panic!("no line"));
    let off = (again.0 - r45.0).dot((r45.1 - r45.0).unit().perp()).abs();
    assert!(off < 0.5, "{off} mm off the reference");
}
