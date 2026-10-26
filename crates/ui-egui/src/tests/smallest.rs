//! The window at the least it can be (`window::MIN_SIZE`, which `main` gives as the window's minimum): nothing runs under, over or
//! past anything else there. Each test runs in the window's real geometry (`titlebar::own`: the root `Ui` is the window).

use egui::{Rect, Shape, pos2, vec2};

use super::titlebar::own;
use super::*;
use crate::window::MIN_SIZE;

fn least() -> egui::Vec2 {
    vec2(MIN_SIZE[0], MIN_SIZE[1])
}

fn window(size: egui::Vec2) -> Rect {
    Rect::from_min_size(pos2(0.0, 0.0), size)
}

/// The cards the window drew: the ribbon, the palettes, the status bar (their ground is the panel colour, and each is wide and tall
/// enough that no button or field is taken for one).
fn cards(h: &Harness<BimApp>) -> Vec<Rect> {
    fn walk(s: &Shape, out: &mut Vec<Rect>) {
        match s {
            Shape::Rect(r) if r.fill == crate::style::pal().panel && r.rect.width() >= 250.0 && r.rect.height() >= 24.0 => out.push(r.rect),
            Shape::Vec(v) => v.iter().for_each(|s| walk(s, out)),
            _ => {}
        }
    }
    let mut out = Vec::new();
    h.output().shapes.iter().for_each(|c| walk(&c.shape, &mut out));
    out
}

/// The window brought down to the least a window can be: from the start, or from a bigger one the palettes were sized in.
fn at_the_least(compact: bool, from_bigger: bool) -> Harness<'static, BimApp> {
    let mut h = own(BOX);
    h.state_mut().compact = compact;
    if from_bigger {
        h.run();
    } else {
        h.set_size(least());
    }
    h.set_size(least());
    h.run();
    h.run();
    h
}

#[test]
fn no_card_runs_under_another_or_off_the_window_at_the_least_a_window_can_be() {
    for compact in [false, true] {
        for from_bigger in [false, true] {
            let h = at_the_least(compact, from_bigger);
            let cards = cards(&h);
            assert!(cards.len() >= 4, "the ribbon, two palettes and the status bar (compact={compact}): {cards:?}");
            let w = window(least());
            for (i, a) in cards.iter().enumerate() {
                assert!(w.expand(0.5).contains_rect(*a), "{a:?} is off the window (compact={compact}, from_bigger={from_bigger})");
                for b in &cards[i + 1..] {
                    let both = a.intersect(*b);
                    assert!(
                        !(both.is_positive() && both.width() > 1.0 && both.height() > 1.0),
                        "{a:?} runs under {b:?} (compact={compact}, from_bigger={from_bigger})"
                    );
                }
            }
        }
    }
}

/// The title the app bar paints (the project and the view), as the rectangle it covers.
fn title_of(h: &Harness<BimApp>) -> Rect {
    fn walk(s: &Shape, out: &mut Vec<Rect>) {
        match s {
            Shape::Text(t) if t.galley.text().contains(" · ") && t.visual_bounding_rect().center().y < 34.0 => {
                out.push(t.visual_bounding_rect())
            }
            Shape::Vec(v) => v.iter().for_each(|s| walk(s, out)),
            _ => {}
        }
    }
    let mut out = Vec::new();
    h.output().shapes.iter().for_each(|c| walk(&c.shape, &mut out));
    out.pop().expect("the app bar's title")
}

#[test]
fn the_title_stays_between_the_quick_access_and_the_caption_buttons_whatever_the_projects_name() {
    let long = "Long ".repeat(40);
    let names = ["House", "A Project With A Rather Long Name For A Small Window, Its Second Phase", long.as_str()];
    for size in [least(), vec2(1280.0, 800.0)] {
        for name in names {
            let mut h = own(BOX);
            h.set_size(size);
            std::sync::Arc::make_mut(&mut h.state_mut().session.model).name = name.to_string();
            h.run();
            h.run();
            let (gear, captions) = (h.get_by_label("Settings").rect(), h.get_by_label("Minimize").rect());
            let title = title_of(&h);
            let said = format!("{name:?} in {size:?}: the title is {title:?}, between the gear's {gear:?} and the buttons' {captions:?}");
            assert!(title.left() >= gear.right() && title.right() <= captions.left(), "{said}");
            if name == "House" && size.x > 1000.0 {
                assert!((title.center().x - size.x / 2.0).abs() < 1.0, "a title that fits is in the window's middle: {said}");
            }
        }
    }
}

#[test]
fn the_tab_just_picked_is_in_view_though_the_tabs_run_past_the_window() {
    let mut h = own(BOX);
    h.set_size(least());
    h.run();
    let in_view = |h: &Harness<BimApp>, label: &str| {
        let (tab, fold) = (h.get_by_label(label).rect(), h.get_by_label("Minimize the Ribbon").rect());
        assert!(
            tab.left() >= 0.0 && tab.right() <= fold.left(),
            "`{label}` {tab:?} is in the tabs, which end where the fold button {fold:?} starts"
        );
    };
    // A tab far along the row.
    h.state_mut().tab = crate::Tab::Manage;
    h.run();
    h.run();
    in_view(&h, "Manage");
    // And a tool's contextual tab, which comes after them all.
    h.state_mut().tab = crate::Tab::Architecture;
    h.run();
    tool(&mut h, "Wall");
    h.run();
    in_view(&h, "Modify | Place Wall");
}

#[test]
fn the_options_bar_stays_clear_of_the_zoom_buttons_and_inside_the_window_at_the_least_a_window_can_be() {
    let mut h = own(BOX);
    h.set_size(least());
    h.run();
    tool(&mut h, "Wall");
    h.run();
    // The bar itself, and each of its fields, are in the window; none of the view's zoom buttons is under any of them.
    let bar = h.state().options_rect;
    let w = window(least());
    assert!(w.contains_rect(bar), "the bar {bar:?} is off the window");
    for (label, role) in [("Height:", Role::Label), ("Location Line:", Role::Label), ("Chain", Role::CheckBox)] {
        let r = h.get_by_role_and_label(role, label).rect();
        assert!(bar.contains_rect(r), "`{label}` {r:?} is out of the bar {bar:?}");
    }
    for label in ["Zoom to Fit", "Zoom In", "Zoom Out"] {
        let button = h.get_by_label(label).rect();
        assert!(!button.intersect(bar).is_positive(), "`{label}` {button:?} is under the Options Bar {bar:?}");
        assert!(w.contains_rect(button), "`{label}` {button:?} is off the window");
    }
}
