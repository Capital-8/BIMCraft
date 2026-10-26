//! What a dialog's draft asks of a list of named entries (patterns, types, styles), in an order the core can do it in. The
//! core refuses a rename onto a name that is taken and a delete of one that something uses, and it is atomic, so the order
//! is the dialog's to get right: a rename chain runs from its end, a swap goes through a spare name, a name freed by a
//! delete is free before a rename takes it, and an entry deleted and made again under its name is changed in place (a delete
//! after the change would take the new one away).

use std::collections::BTreeSet;

/// A row of the draft: the name the entry had (none, new), the name it has, and whether it was deleted.
pub(crate) struct Row<'a> {
    pub was: Option<&'a str>,
    pub name: &'a str,
    pub gone: bool,
}

/// What to ask of the project: renames `(from, to)` first, in order; then, after whatever refers to the entries is changed,
/// the names to delete.
#[derive(Debug, Default, PartialEq)]
pub(crate) struct Plan {
    pub renames: Vec<(String, String)>,
    pub deletes: Vec<String>,
}

/// The plan for `rows`, over the entries that `existing` names. The live rows' names are distinct (the dialog checks that).
pub(crate) fn plan(existing: &BTreeSet<String>, rows: &[Row]) -> Plan {
    let live = |n: &str| rows.iter().find(|r| !r.gone && r.name == n);
    // Every name that is, or will be, in use: a spare name is none of them.
    let mut taken = existing.clone();
    taken.extend(rows.iter().map(|r| r.name.to_string()));
    let mut renames: Vec<(String, String)> = rows
        .iter()
        .filter(|r| !r.gone)
        .filter_map(|r| r.was.filter(|w| *w != r.name).map(|w| (w.to_string(), r.name.to_string())))
        .collect();
    let mut deletes = Vec::new();
    for was in rows.iter().filter(|r| r.gone).filter_map(|r| r.was) {
        match live(was) {
            // Made again under its name: the new row changes it in place.
            Some(r) if r.was.is_none() => {}
            // A live row is renamed onto it: it steps aside under a spare name and is deleted at the end.
            Some(_) => {
                let spare = spare(&taken, &format!("{was} (deleted)"));
                taken.insert(spare.clone());
                renames.push((was.to_string(), spare.clone()));
                deletes.push(spare);
            }
            None => deletes.push(was.to_string()),
        }
    }
    Plan { renames: order(existing, taken, renames), deletes }
}

/// `base`, or `base 2`, `base 3`… the first that `taken` doesn't hold.
fn spare(taken: &BTreeSet<String>, base: &str) -> String {
    std::iter::once(base.to_string()).chain((2..).map(|i| format!("{base} {i}"))).find(|n| !taken.contains(n)).unwrap_or_default()
}

/// `renames` in an order each can be done in: one whose new name is free goes first, and when each waits on another (a swap
/// is the shortest such loop) one steps aside under a spare name and takes its own after.
fn order(existing: &BTreeSet<String>, mut taken: BTreeSet<String>, renames: Vec<(String, String)>) -> Vec<(String, String)> {
    let mut held = existing.clone();
    let mut pending = renames;
    let mut out = Vec::new();
    while !pending.is_empty() {
        let sources: BTreeSet<&str> = pending.iter().map(|(from, _)| from.as_str()).collect();
        // A name held by nothing that moves is no loop: that rename goes as it is, and the core says why it can't.
        let free = pending.iter().position(|(_, to)| !held.contains(to) || !sources.contains(to.as_str()));
        let (from, to) = pending.remove(free.unwrap_or(0));
        let (from, to) = match free {
            Some(_) => (from, to),
            None => {
                let aside = spare(&taken, &format!("{from} (renaming)"));
                taken.insert(aside.clone());
                pending.push((aside.clone(), to));
                (from, aside)
            }
        };
        held.remove(&from);
        held.insert(to.clone());
        out.push((from, to));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(names: &[&str]) -> BTreeSet<String> {
        names.iter().map(|n| n.to_string()).collect()
    }

    fn row<'a>(was: Option<&'a str>, name: &'a str, gone: bool) -> Row<'a> {
        Row { was, name, gone }
    }

    /// Runs `renames` on `names` as the core does: a rename onto a name that is taken is refused.
    fn run(names: &BTreeSet<String>, renames: &[(String, String)]) -> BTreeSet<String> {
        let mut now = names.clone();
        for (from, to) in renames {
            assert!(now.remove(from), "no {from} to rename in {now:?}");
            assert!(now.insert(to.clone()), "{to} is taken in {now:?}");
        }
        now
    }

    #[test]
    fn a_chain_runs_from_its_end_and_a_swap_goes_through_a_spare_name() {
        let existing = set(&["A", "B", "C"]);
        let chain = plan(&existing, &[row(Some("A"), "B", false), row(Some("B"), "C", false), row(Some("C"), "D", false)]);
        assert_eq!(run(&existing, &chain.renames), set(&["B", "C", "D"]));
        assert_eq!(chain.renames.len(), 3, "no spare name for a chain: {:?}", chain.renames);
        let swap = plan(&existing, &[row(Some("A"), "B", false), row(Some("B"), "A", false), row(Some("C"), "C", false)]);
        assert_eq!(run(&existing, &swap.renames), existing);
        assert_eq!(swap.renames.len(), 3, "a swap takes a spare name: {:?}", swap.renames);
        // A loop of three.
        let ring = plan(&existing, &[row(Some("A"), "B", false), row(Some("B"), "C", false), row(Some("C"), "A", false)]);
        assert_eq!(run(&existing, &ring.renames), existing);
    }

    #[test]
    fn an_entry_deleted_and_made_again_is_changed_in_place_and_a_name_a_rename_wants_is_freed() {
        let existing = set(&["A", "B"]);
        let again = plan(&existing, &[row(Some("A"), "A", true), row(None, "A", false), row(Some("B"), "B", false)]);
        assert_eq!(again, Plan::default(), "no delete after the new one is made");
        // B is deleted and A takes its name: B steps aside, A takes the name, the spare goes at the end.
        let onto = plan(&existing, &[row(Some("A"), "B", false), row(Some("B"), "B", true)]);
        assert_eq!(run(&existing, &onto.renames), set(&["B", "B (deleted)"]));
        assert_eq!(onto.deletes, ["B (deleted)"]);
        let plain = plan(&existing, &[row(Some("A"), "A", false), row(Some("B"), "B", true)]);
        assert_eq!((plain.renames.len(), plain.deletes.as_slice()), (0, ["B".to_string()].as_slice()));
    }

    #[test]
    fn a_spare_name_is_never_one_a_row_ends_with() {
        let existing = set(&["A", "B"]);
        // B is deleted and A takes its name; a new row is named as B's spare would be.
        let p = plan(&existing, &[row(Some("A"), "B", false), row(Some("B"), "B", true), row(None, "B (deleted)", false)]);
        assert_eq!((p.renames.len(), p.deletes.as_slice()), (2, ["B (deleted) 2".to_string()].as_slice()), "{p:?}");
        // A swap, with a new row named as its spare would be.
        let swap = plan(&existing, &[row(Some("A"), "B", false), row(Some("B"), "A", false), row(None, "A (renaming)", false)]);
        assert!(swap.renames.iter().all(|(_, to)| to != "A (renaming)"), "{swap:?}");
        assert_eq!(run(&existing, &swap.renames), existing);
    }

    #[test]
    fn a_name_nothing_frees_is_left_to_the_core_to_refuse() {
        let existing = set(&["A", "B"]);
        let p = plan(&existing, &[row(Some("A"), "B", false)]);
        assert_eq!(p.renames, [("A".to_string(), "B".to_string())]);
    }
}
