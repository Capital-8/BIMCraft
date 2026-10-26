//! Sketches: Revit's sketch mode, the one way an outline is drawn for a floor, a ceiling, a roof's footprint, a shaft, and
//! what's built on them later (filled regions, topography).
//!
//! A sketch is lines and arcs ([`SketchLine`]), drawn in any order and either way round; the element they draw keeps them,
//! each line a part with an id and a GUID of its own. [`solve`] joins them, end to end within [`TOL`], into closed loops and
//! nests the loops: a loop inside no other is an outline, one inside an outline a hole in it, one inside a hole an island,
//! an outline again. What can't be solved it names, every problem at once and the lines it's about, so the lines can be
//! shown to whoever drew them: a line too short or out of range, two lines crossing or one ending on another, two lines
//! along each other, an end no other line meets, three or more lines meeting at a point.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::curves::Curve;
use crate::geom::{self, P2, Region};
use crate::ids::{Guid, Id};
use crate::model::{MAX_COORD, MAX_POINTS, TOL};

/// A straight line, from one end to the other.
pub type Line = [P2; 2];

/// A sketch line: any [`Curve`], a line, an arc, a polyline, a circle, an ellipse or a spline. Its id and GUID are its
/// own, given when what it draws is made or edited and kept while it stays in the sketch. In a file and a command it is
/// one flat object: its `id` and `guid` and its curve's fields.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "RawSketchLine", into = "RawSketchLine")]
pub struct SketchLine {
    pub id: Id,
    pub guid: Guid,
    pub curve: Curve,
    /// A filled region's boundary line drawn in a line style of its own, else in the region's.
    pub style: Option<String>,
    /// A filled region's boundary line not drawn at all: Revit's Invisible lines, for this one.
    pub invisible: bool,
}

crate::curve_struct! {
    /// A sketch line as a file and a command hold it.
    RawSketchLine {
        #[serde(default, skip_serializing_if = "crate::ids::unset")]
        id: Id,
        #[serde(default, skip_serializing_if = "Guid::is_nil")]
        guid: Guid,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        style: Option<String>,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        invisible: bool,
    }
}

impl TryFrom<RawSketchLine> for SketchLine {
    type Error = String;
    fn try_from(r: RawSketchLine) -> Result<SketchLine, String> {
        Ok(SketchLine { id: r.id, guid: r.guid, curve: r.fields().curve()?, style: r.style, invisible: r.invisible })
    }
}

impl From<SketchLine> for RawSketchLine {
    fn from(l: SketchLine) -> RawSketchLine {
        let mut r = RawSketchLine { id: l.id, guid: l.guid, style: l.style, invisible: l.invisible, ..RawSketchLine::default() };
        r.set(&l.curve);
        r
    }
}

impl From<Line> for SketchLine {
    fn from([start, end]: Line) -> SketchLine {
        SketchLine::new(start, end)
    }
}

impl From<Curve> for SketchLine {
    fn from(curve: Curve) -> SketchLine {
        SketchLine { id: 0, guid: Guid::default(), curve, style: None, invisible: false }
    }
}

impl SketchLine {
    /// A straight line, with no id yet.
    pub fn new(start: P2, end: P2) -> SketchLine {
        Curve::line(start, end).into()
    }

    /// An arc from `start` to `end` through `through`, with no id yet; straight when `through` is no further than
    /// [`TOL`] off the line between them.
    pub fn arc(start: P2, end: P2, through: P2) -> SketchLine {
        Curve::arc_through(start, end, through).into()
    }

    /// Where it starts.
    pub fn start(&self) -> P2 {
        self.curve.start()
    }

    /// Where it ends: where it starts, when it's closed.
    pub fn end(&self) -> P2 {
        self.curve.end()
    }

    /// Its points from start to end, as drawn: see [`Curve::points`].
    pub fn points(&self) -> Vec<P2> {
        self.curve.points()
    }

    /// Its middle: half way along it.
    pub fn middle(&self) -> P2 {
        self.curve.middle()
    }

    /// How far `p` is from it.
    pub fn dist(&self, p: P2) -> f64 {
        self.curve.dist(p)
    }

    /// Each point it holds, moved by `f`.
    pub fn map(&self, f: impl Fn(P2) -> P2) -> SketchLine {
        SketchLine { curve: self.curve.map(f), ..self.clone() }
    }

    fn in_range(&self) -> bool {
        self.curve.in_range()
    }
}

/// The most lines a sketch may hold.
pub const MAX_LINES: usize = MAX_POINTS;

/// The most straight pieces a sketch's lines may be drawn as: an arc, a circle or a curve is solved as the chords that draw it
/// ([`Curve::drawn_len`]), up to 8,193 for a circle. Past it a sketch is refused whole, before any of it is worked on.
pub const MAX_PIECES: usize = 200_000;

/// The most steps [`solve`] takes finding which pieces meet: past it the pieces lie too close together, long and slanted
/// (ten thousand parallel diagonals), to tell without trying every pair, and the sketch is refused rather than waited for.
const MAX_WORK: usize = 10_000_000;

/// The most steps [`solve`] takes nesting the loops, each a point of a loop looked at while finding which loops hold which.
const MAX_NEST: usize = 50_000_000;

/// The most problems [`solve`] names: past that, the first ones are enough to start fixing.
pub const MAX_PROBLEMS: usize = 100;

/// Why lines don't make loops, and the lines it's about, numbered from 0 in the order given.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "problem", rename_all = "snake_case")]
pub enum Problem {
    /// No lines at all.
    Empty,
    /// More than a sketch can be solved with: `count` of `what`, which is "lines" (more than [`MAX_LINES`]), "pieces" (more
    /// than [`MAX_PIECES`]), "steps" (finding which pieces meet takes more than a sketch is given) or "loops" (nesting them does).
    TooMany { count: usize, what: &'static str },
    /// An end that isn't a number, or is further out than [`MAX_COORD`].
    OutOfRange { line: usize },
    /// A line [`TOL`] long or less.
    TooShort { line: usize },
    /// A line that isn't one: points one on another, a spline's knots out of order, an arc turning a whole circle.
    Malformed { line: usize, why: String },
    /// Two lines crossing, or one ending part way along the other, at `at`; a line twice, when it crosses itself.
    Cross { lines: [usize; 2], at: P2 },
    /// Two lines lying along each other for more than [`TOL`].
    Overlap { lines: [usize; 2] },
    /// An end, at `at`, that no other line meets: the loop is open there.
    Open { line: usize, at: P2 },
    /// Three or more lines meeting at `at`: a loop goes through a point once.
    Branch { lines: Vec<usize>, at: P2 },
    /// A loop enclosing no area.
    NoArea { lines: Vec<usize> },
    /// More than one outline, where one is wanted: `count` of them.
    Separate { count: usize },
}

impl Problem {
    /// The lines it's about, to show.
    pub fn lines(&self) -> Vec<usize> {
        match self {
            Problem::Empty | Problem::TooMany { .. } | Problem::Separate { .. } => Vec::new(),
            Problem::OutOfRange { line } | Problem::TooShort { line } | Problem::Malformed { line, .. } | Problem::Open { line, .. } => {
                vec![*line]
            }
            // A line crossing or overlapping itself is one line, said once.
            Problem::Cross { lines, .. } | Problem::Overlap { lines } if lines[0] == lines[1] => vec![lines[0]],
            Problem::Cross { lines, .. } | Problem::Overlap { lines } => lines.to_vec(),
            Problem::Branch { lines, .. } | Problem::NoArea { lines } => lines.clone(),
        }
    }

    /// Where it is, when it's at a point.
    pub fn at(&self) -> Option<P2> {
        match self {
            Problem::Cross { at, .. } | Problem::Open { at, .. } | Problem::Branch { at, .. } => Some(*at),
            _ => None,
        }
    }

    /// Which to fix first: what's wrong with a line itself, then lines that cross or lie along each other (which often
    /// leave ends open too), then where loops branch or stay open.
    pub fn rank(&self) -> u8 {
        match self {
            Problem::Empty
            | Problem::TooMany { .. }
            | Problem::OutOfRange { .. }
            | Problem::TooShort { .. }
            | Problem::Malformed { .. } => 0,
            Problem::Cross { .. } => 1,
            Problem::Overlap { .. } => 2,
            Problem::Branch { .. } => 3,
            Problem::Open { .. } => 4,
            Problem::NoArea { .. } => 5,
            Problem::Separate { .. } => 6,
        }
    }

    /// It said in a sentence, each line as `name` names it (`line 999812`, `edge 2`).
    pub fn said(&self, name: &dyn Fn(usize) -> String) -> String {
        let at = |p: &P2| format!("[{}, {}]", p.0.round(), p.1.round());
        let list = |ls: &[usize]| {
            let names: Vec<String> = ls.iter().map(|&i| name(i)).collect();
            match names.split_last() {
                Some((last, rest)) if !rest.is_empty() => format!("{} and {last}", rest.join(", ")),
                _ => names.join(""),
            }
        };
        match self {
            Problem::Empty => "there are no lines: draw the outline".into(),
            Problem::TooMany { count, what: "lines" } => format!("{count} lines, more than {MAX_LINES}"),
            Problem::TooMany { count, what: "pieces" } => format!(
                "its lines are drawn as {count} straight pieces (arcs, circles and curves are cut into chords), more than {MAX_PIECES}: draw fewer, or smaller"
            ),
            Problem::TooMany { count, what: "steps" } => format!(
                "its pieces lie too close together to tell which cross ({count} pairs of them looked at, more than {MAX_WORK}): draw fewer lines, or farther apart"
            ),
            Problem::TooMany { count, .. } => {
                format!("the loops are nested too deeply to solve ({count} steps, more than {MAX_NEST}): draw fewer")
            }
            Problem::OutOfRange { line } => {
                format!("{} has a point that isn't a number within {MAX_COORD} mm, or a curve reaching further", name(*line))
            }
            Problem::TooShort { line } => format!("{} is {TOL} mm long or less", name(*line)),
            Problem::Malformed { line, why } => format!("{} isn't a line it can draw: {why}", name(*line)),
            Problem::Cross { lines, at: p } if lines[0] == lines[1] => format!("{} crosses itself at {}", name(lines[0]), at(p)),
            Problem::Overlap { lines } if lines[0] == lines[1] => format!("{} lies along itself", name(lines[0])),
            Problem::Cross { lines, at: p } => format!("{} intersect at {}", list(lines), at(p)),
            Problem::Overlap { lines } => format!("{} overlap", list(lines)),
            Problem::Open { line, at: p } => format!("{} ends at {} without meeting another: the loop is open", name(*line), at(p)),
            Problem::Branch { lines, at: p } => {
                // One line coming back to a point (a bend within a millimetre of another place on it, a loop in it).
                let mut once = lines.clone();
                once.dedup();
                match once.as_slice() {
                    [one] => format!("{} passes {} more than once: a loop passes a point once", name(*one), at(p)),
                    _ => format!("{} meet at {}: a loop passes a point once", list(&once), at(p)),
                }
            }
            Problem::NoArea { lines } => format!("the loop of {} encloses no area", list(lines)),
            Problem::Separate { count } => format!("the lines make {count} separate outlines, where one is wanted"),
        }
    }
}

impl fmt::Display for Problem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.said(&|i| format!("line {i}")))
    }
}

/// Problems said in one line, the first few named, each line as `name` names it.
pub fn say(problems: &[Problem], name: &dyn Fn(usize) -> String) -> String {
    let shown: Vec<String> = problems.iter().take(5).map(|p| p.said(name)).collect();
    match problems.len() {
        n if n > shown.len() => format!("{}; and {} more", shown.join("; "), n - shown.len()),
        _ => shown.join("; "),
    }
}

/// How a sketch's lines are named to whoever drew them: by id, or by place in the list (from 0) when one has none yet.
pub fn namer(lines: &[SketchLine]) -> impl Fn(usize) -> String + '_ {
    |i| match lines.get(i) {
        Some(l) if !crate::ids::unset(&l.id) => format!("line {}", l.id),
        _ => format!("line {i}"),
    }
}

/// What a sketch's lines enclose: each outline (counter-clockwise) with its holes (clockwise), in the order their first
/// lines were drawn. An island in a hole is an outline of its own.
#[derive(Clone, Debug, PartialEq)]
pub struct Profile {
    pub regions: Vec<Region>,
}

impl Profile {
    /// Its area: the outlines' less their holes'.
    pub fn area(&self) -> f64 {
        self.regions.iter().map(Region::area).sum()
    }
}

/// The lines solved into loops, nested into regions; or every problem found (up to [`MAX_PROBLEMS`]), what to fix first
/// first ([`Problem::rank`]).
pub fn solve(lines: &[SketchLine]) -> Result<Profile, Vec<Problem>> {
    if lines.is_empty() {
        return Err(vec![Problem::Empty]);
    }
    if lines.len() > MAX_LINES {
        return Err(vec![Problem::TooMany { count: lines.len(), what: "lines" }]);
    }
    // Each line on its own first: its points are numbers, its ends apart.
    let mut problems: Vec<Problem> = Vec::new();
    for (i, l) in lines.iter().enumerate() {
        if !l.in_range() {
            problems.push(Problem::OutOfRange { line: i });
        } else if let Err(why) = l.curve.sound("it") {
            problems.push(Problem::Malformed { line: i, why: why.trim_start_matches("it: ").to_string() });
        } else if l.curve.length() <= TOL {
            problems.push(Problem::TooShort { line: i });
        }
    }
    if !problems.is_empty() {
        return Err(cap(problems));
    }
    // Not too many pieces, before any is made: a line is as many as it is drawn through points less one.
    let count: usize = lines.iter().map(|l| l.curve.drawn_len().saturating_sub(1)).sum();
    if count > MAX_PIECES {
        return Err(vec![Problem::TooMany { count, what: "pieces" }]);
    }
    // Lines as straight pieces (an arc as its chords), each knowing its line and the points at its ends; ends within TOL of
    // each other are one point.
    let mut welder = Welder::default();
    let mut pieces: Vec<(Line, usize)> = Vec::with_capacity(count);
    let mut ends: Vec<[usize; 2]> = Vec::with_capacity(count);
    for (i, l) in lines.iter().enumerate() {
        let pts = l.points();
        // A bend a line was drawn with that is shorter than the pieces can tell apart: say where, to be drawn as one corner.
        if let (true, Some(w)) = (l.curve.is_poly(), pts.windows(2).find(|w| w[0].dist(w[1]) <= TOL)) {
            let why = format!("it has two points less than {TOL} mm apart, at [{}, {}]: draw them as one", w[0].0.round(), w[0].1.round());
            problems.push(Problem::Malformed { line: i, why });
            continue;
        }
        let nodes: Vec<usize> = pts.iter().map(|p| welder.weld(*p)).collect();
        for (w, n) in pts.windows(2).zip(nodes.windows(2)) {
            // A chord of a small curve that welds to a point is no piece.
            if n[0] != n[1] {
                pieces.push(([w[0], w[1]], i));
                ends.push([n[0], n[1]]);
            }
        }
    }
    if !problems.is_empty() {
        return Err(cap(problems));
    }
    let points = welder.points;
    // Each pair of pieces that could touch (their boxes, grown by TOL, overlap), and how they do: of one line too, which
    // can cross itself, the next piece of it meeting this one at a point being right. Of the problems a pair of lines has
    // of a kind, the one at the pieces first in the sketch is named.
    let tree = Boxes::new(
        pieces
            .iter()
            .map(|(l, _)| [l[0].0.min(l[1].0) - TOL, l[0].1.min(l[1].1) - TOL, l[0].0.max(l[1].0) + TOL, l[0].1.max(l[1].1) + TOL])
            .collect(),
    );
    let mut found: std::collections::BTreeMap<([usize; 2], u8), (usize, usize, Problem)> = std::collections::BTreeMap::new();
    let mut work = 0;
    let walked = tree.pairs(&mut work, |i, j| {
        let (p, q) = if (pieces[i].1, i) <= (pieces[j].1, j) { (i, j) } else { (j, i) };
        let pair = [pieces[p].1, pieces[q].1];
        if let Some(problem) = meet(pieces[p].0, pieces[q].0, ends[p], ends[q], pair) {
            let key = (pair, problem.rank());
            match found.get_mut(&key) {
                Some(was) if (p, q) < (was.0, was.1) => *was = (p, q, problem),
                Some(_) => {}
                None => {
                    found.insert(key, (p, q, problem));
                }
            }
        }
        found.len() <= MAX_PROBLEMS
    });
    let Ok(walked) = walked else { return Err(vec![Problem::TooMany { count: work, what: "steps" }]) };
    problems.extend(found.into_values().map(|(_, _, problem)| problem));
    if !walked {
        problems.sort_by_key(|p| (p.rank(), p.lines().first().copied().unwrap_or(0)));
        return Err(cap(problems));
    }
    // Two pieces at every point: one, it's open; three or more, it branches.
    let mut first = vec![0usize; points.len() + 1];
    for e in &ends {
        first[e[0] + 1] += 1;
        first[e[1] + 1] += 1;
    }
    for n in 0..points.len() {
        first[n + 1] += first[n];
    }
    let mut at = vec![0usize; first[points.len()]];
    let mut put = first.clone();
    for (i, e) in ends.iter().enumerate() {
        for &n in e {
            at[put[n]] = i;
            put[n] += 1;
        }
    }
    let at_point = |n: usize| &at[first[n]..first[n + 1]];
    let (mut opens, mut branches) = (0, 0);
    for n in 0..points.len() {
        match at_point(n) {
            [] | [_, _] => {}
            &[piece] if opens <= MAX_PROBLEMS => {
                opens += 1;
                problems.push(Problem::Open { line: pieces[piece].1, at: points[n] });
            }
            &[_] => {}
            ps if branches <= MAX_PROBLEMS => {
                branches += 1;
                let mut ls: Vec<usize> = ps.iter().map(|&p| pieces[p].1).collect();
                ls.sort_unstable();
                problems.push(Problem::Branch { lines: ls, at: points[n] });
            }
            _ => {}
        }
    }
    if !problems.is_empty() {
        problems.sort_by_key(|p| (p.rank(), p.lines().first().copied().unwrap_or(0)));
        return Err(cap(problems));
    }
    // Every point has two pieces: each part of the sketch is one loop, walked from its first piece the way it was drawn.
    let mut used = vec![false; pieces.len()];
    let mut loops: Vec<(Vec<P2>, Vec<usize>)> = Vec::new();
    for first_piece in 0..pieces.len() {
        if used[first_piece] {
            continue;
        }
        let (mut ring, mut members) = (Vec::new(), Vec::new());
        let (mut piece, mut from) = (first_piece, ends[first_piece][0]);
        while !used[piece] {
            used[piece] = true;
            if members.last() != Some(&pieces[piece].1) && !members.contains(&pieces[piece].1) {
                members.push(pieces[piece].1);
            }
            ring.push(points[from]);
            let to = if ends[piece][0] == from { ends[piece][1] } else { ends[piece][0] };
            from = to;
            piece = at_point(to).iter().copied().find(|&p| p != piece).unwrap_or(piece);
        }
        if geom::area(&ring).abs() <= TOL * TOL {
            problems.push(Problem::NoArea { lines: members.clone() });
        }
        loops.push((ring, members));
    }
    if !problems.is_empty() {
        return Err(cap(problems));
    }
    // No two loops cross or touch, so a loop is inside another when any of its points is: how many it's inside says
    // whether it's an outline (none, or an even number) or a hole (odd), in the innermost loop around it. Only the loops
    // whose box holds the point are looked into.
    let bounds = |ring: &Vec<P2>| {
        ring.iter().fold([f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY], |b, p| {
            [b[0].min(p.0), b[1].min(p.1), b[2].max(p.0), b[3].max(p.1)]
        })
    };
    let nest = Boxes::new(loops.iter().map(|(ring, _)| bounds(ring)).collect());
    let mut inside: Vec<Vec<usize>> = vec![Vec::new(); loops.len()];
    let mut steps = 0;
    for (i, (ring, _)) in loops.iter().enumerate() {
        let p = ring[0];
        nest.at(p, |j| {
            if j != i {
                steps += loops[j].0.len();
                if steps <= MAX_NEST && geom::contains(&loops[j].0, p) {
                    inside[i].push(j);
                }
            }
            steps <= MAX_NEST
        });
        if steps > MAX_NEST {
            return Err(vec![Problem::TooMany { count: steps, what: "loops" }]);
        }
    }
    let mut regions: Vec<Region> = Vec::new();
    let mut region_of = vec![usize::MAX; loops.len()];
    for (i, (ring, _)) in loops.iter().enumerate() {
        if inside[i].len().is_multiple_of(2) {
            region_of[i] = regions.len();
            regions.push(Region { outer: wound(ring, true), holes: Vec::new() });
        }
    }
    for (i, (ring, _)) in loops.iter().enumerate() {
        if !inside[i].len().is_multiple_of(2)
            && let Some(&o) = inside[i].iter().find(|&&j| inside[j].len() + 1 == inside[i].len())
            && let Some(r) = regions.get_mut(region_of[o])
        {
            r.holes.push(wound(ring, false));
        }
    }
    Ok(Profile { regions })
}

/// The lines solved into the one outline (with its holes) a floor, a ceiling or a roof takes.
pub fn solve_one(lines: &[SketchLine]) -> Result<Region, Vec<Problem>> {
    let mut p = solve(lines)?;
    match p.regions.len() {
        1 => Ok(p.regions.remove(0)),
        count => Err(vec![Problem::Separate { count }]),
    }
}

/// An outline and its holes as sketch lines, to edit: each ring's edges in order, straight, with no ids yet.
pub fn lines_of(outline: &[P2], holes: &[Vec<P2>]) -> Vec<SketchLine> {
    std::iter::once(outline)
        .chain(holes.iter().map(Vec::as_slice))
        .flat_map(|r| geom::ring_edges(r).map(|(a, b)| SketchLine::new(a, b)))
        .collect()
}

/// Whether `lines` solve to `outline` and `holes`, each ring the same points round in the same order from any start, either
/// way round: a sketch kept with an element is still what it draws.
pub fn draws(lines: &[SketchLine], outline: &[P2], holes: &[Vec<P2>]) -> bool {
    let same = |a: &[P2], b: &[P2]| {
        let n = a.len();
        let close = |p: P2, q: P2| p.dist(q) <= TOL;
        n == b.len()
            && n > 0
            && (0..n).any(|k| (0..n).all(|i| close(a[i], b[(i + k) % n])) || (0..n).all(|i| close(a[i], b[(k + n - i) % n])))
    };
    match solve_one(lines) {
        Ok(r) => same(&r.outer, outline) && r.holes.len() == holes.len() && r.holes.iter().all(|h| holes.iter().any(|g| same(h, g))),
        Err(_) => false,
    }
}

/// A kept sketch's lines checked as [`crate::model::Model::check`] checks what a file holds: no more than [`MAX_LINES`],
/// every point a number within range, and no more than [`MAX_PIECES`] drawn between them.
pub fn check(who: &str, lines: &[SketchLine]) -> Result<(), String> {
    if lines.len() > MAX_LINES {
        return Err(format!("{who}: its sketch has {} lines, more than {MAX_LINES}", lines.len()));
    }
    if let Some(i) = lines.iter().position(|l| l.curve.count() > MAX_POINTS) {
        return Err(format!("{who}: sketch line {i} has more than {MAX_POINTS} points"));
    }
    if let Some(i) = lines.iter().position(|l| !l.in_range()) {
        return Err(format!("{who}: sketch line {i} has a point that isn't a number within {MAX_COORD} mm, or a curve reaching further"));
    }
    let pieces: usize = lines.iter().map(|l| l.curve.drawn_len().saturating_sub(1)).sum();
    if pieces > MAX_PIECES {
        return Err(format!("{who}: its sketch is drawn as {pieces} straight pieces, more than {MAX_PIECES}"));
    }
    Ok(())
}

/// Points made one where they lie within [`TOL`] of each other: a grid of cells [`TOL`] across holds them, so a point is
/// looked for among those in its own cell and the eight round it, not among all.
#[derive(Default)]
struct Welder {
    points: Vec<P2>,
    cells: std::collections::HashMap<(i64, i64), Vec<usize>>,
}

impl Welder {
    /// The first point within [`TOL`] of `p` (by when it was made), or `p` as a new one.
    fn weld(&mut self, p: P2) -> usize {
        let cell = |v: f64| (v / TOL).floor() as i64;
        let (cx, cy) = (cell(p.0), cell(p.1));
        let mut found: Option<usize> = None;
        for x in cx - 1..=cx + 1 {
            for y in cy - 1..=cy + 1 {
                for &n in self.cells.get(&(x, y)).into_iter().flatten() {
                    if found.is_none_or(|f| n < f) && self.points[n].dist(p) <= TOL {
                        found = Some(n);
                    }
                }
            }
        }
        found.unwrap_or_else(|| {
            self.points.push(p);
            self.cells.entry((cx, cy)).or_default().push(self.points.len() - 1);
            self.points.len() - 1
        })
    }
}

/// Boxes `[x0, y0, x1, y1]` in a tree (halved again and again along their longer side, a few to a leaf), to find which
/// overlap which, and which hold a point, without trying them all.
struct Boxes {
    boxes: Vec<[f64; 4]>,
    order: Vec<usize>,
    nodes: Vec<Node>,
}

/// A node of [`Boxes`]: the box round `order[from..to]`, and its two halves unless it is a leaf.
struct Node {
    bounds: [f64; 4],
    from: usize,
    to: usize,
    kids: Option<[usize; 2]>,
}

/// How many boxes a leaf of [`Boxes`] holds, at most.
const LEAF: usize = 8;

fn overlap(a: &[f64; 4], b: &[f64; 4]) -> bool {
    a[0] <= b[2] && b[0] <= a[2] && a[1] <= b[3] && b[1] <= a[3]
}

impl Boxes {
    fn new(boxes: Vec<[f64; 4]>) -> Boxes {
        let mut order: Vec<usize> = (0..boxes.len()).collect();
        let mut nodes = Vec::new();
        if !boxes.is_empty() {
            Boxes::build(&boxes, &mut order, 0, &mut nodes);
        }
        Boxes { boxes, order, nodes }
    }

    /// The node for `order[from..from + len]`, which is `slice`, and the ones under it: the index of the first.
    fn build(boxes: &[[f64; 4]], slice: &mut [usize], from: usize, nodes: &mut Vec<Node>) -> usize {
        let bounds = slice.iter().fold([f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY], |b, &i| {
            [b[0].min(boxes[i][0]), b[1].min(boxes[i][1]), b[2].max(boxes[i][2]), b[3].max(boxes[i][3])]
        });
        let id = nodes.len();
        nodes.push(Node { bounds, from, to: from + slice.len(), kids: None });
        if slice.len() > LEAF {
            // Halved at the middle one along the longer side.
            let axis = usize::from(bounds[3] - bounds[1] > bounds[2] - bounds[0]);
            let centre = |i: usize| boxes[i][axis] + boxes[i][axis + 2];
            let mid = slice.len() / 2;
            slice.select_nth_unstable_by(mid, |&a, &b| centre(a).total_cmp(&centre(b)));
            let (low, high) = slice.split_at_mut(mid);
            let l = Boxes::build(boxes, low, from, nodes);
            let r = Boxes::build(boxes, high, from + mid, nodes);
            nodes[id].kids = Some([l, r]);
        }
        id
    }

    /// Calls `f(i, j)` for each pair of boxes that overlap, once, until it says `false`. Ok(true) when all were looked at,
    /// Ok(false) when `f` stopped it; Err when it took more than [`MAX_WORK`] steps, `work` counting them.
    fn pairs(&self, work: &mut usize, mut f: impl FnMut(usize, usize) -> bool) -> Result<bool, ()> {
        let mut stack = if self.nodes.is_empty() { Vec::new() } else { vec![(0, 0)] };
        while let Some((a, b)) = stack.pop() {
            *work += 1;
            if *work > MAX_WORK {
                return Err(());
            }
            let (na, nb) = (&self.nodes[a], &self.nodes[b]);
            if a != b && !overlap(&na.bounds, &nb.bounds) {
                continue;
            }
            match (na.kids, nb.kids) {
                (None, None) => {
                    let (xs, ys) = (&self.order[na.from..na.to], &self.order[nb.from..nb.to]);
                    for (k, &i) in xs.iter().enumerate() {
                        // The same leaf: each pair once.
                        for &j in if a == b { &xs[k + 1..] } else { ys } {
                            *work += 1;
                            if overlap(&self.boxes[i], &self.boxes[j]) && !f(i, j) {
                                return Ok(false);
                            }
                        }
                    }
                }
                (Some([l, r]), _) if a == b => stack.extend([(l, l), (r, r), (l, r)]),
                (Some([l, r]), None) => stack.extend([(l, b), (r, b)]),
                (None, Some([l, r])) => stack.extend([(a, l), (a, r)]),
                (Some([l, r]), Some([m, n])) => {
                    // The bigger one halved.
                    let area = |x: &[f64; 4]| (x[2] - x[0]) * (x[3] - x[1]);
                    if area(&na.bounds) >= area(&nb.bounds) {
                        stack.extend([(l, b), (r, b)]);
                    } else {
                        stack.extend([(a, m), (a, n)]);
                    }
                }
            }
        }
        Ok(true)
    }

    /// Calls `f(i)` for each box that holds `p`, until it says `false`.
    fn at(&self, p: P2, mut f: impl FnMut(usize) -> bool) {
        let holds = |b: &[f64; 4]| b[0] <= p.0 && p.0 <= b[2] && b[1] <= p.1 && p.1 <= b[3];
        let mut stack = if self.nodes.is_empty() { Vec::new() } else { vec![0] };
        while let Some(n) = stack.pop() {
            let node = &self.nodes[n];
            if !holds(&node.bounds) {
                continue;
            }
            match node.kids {
                Some(kids) => stack.extend(kids),
                None => {
                    for &i in &self.order[node.from..node.to] {
                        if holds(&self.boxes[i]) && !f(i) {
                            return;
                        }
                    }
                }
            }
        }
    }
}

/// How two straight pieces (of lines `pair`, given in order) go wrong where they meet, if they do: along each other,
/// crossing, or one ending part way along the other. Meeting end to end is right.
fn meet([a, b]: Line, [c, d]: Line, e: [usize; 2], f: [usize; 2], pair: [usize; 2]) -> Option<Problem> {
    if (e[0] == f[0] && e[1] == f[1]) || (e[0] == f[1] && e[1] == f[0]) {
        return Some(Problem::Overlap { lines: pair });
    }
    // Along the same line: overlapping when what they share of it is longer than TOL.
    let off = |p: P2, s: P2, t: P2| (t - s).unit().cross(p - s).abs();
    if off(c, a, b) <= TOL && off(d, a, b) <= TOL {
        let u = (b - a).unit();
        let (s, t) = ((c - a).dot(u), (d - a).dot(u));
        let shared = s.max(t).min(a.dist(b)) - s.min(t).max(0.0);
        return (shared > TOL).then_some(Problem::Overlap { lines: pair });
    }
    // An end on the other piece, away from its ends.
    let on = |p: P2, s: P2, t: P2| geom::dist_to_segment(p, s, t) <= TOL && p.dist(s) > TOL && p.dist(t) > TOL;
    if let Some(p) = [c, d].into_iter().find(|&p| on(p, a, b)).or_else(|| [a, b].into_iter().find(|&p| on(p, c, d))) {
        return Some(Problem::Cross { lines: pair, at: p });
    }
    let x = a + (b - a) * geom::crossing(a, b, c, d)?;
    [a, b, c, d].iter().all(|p| p.dist(x) > TOL).then_some(Problem::Cross { lines: pair, at: x })
}

/// `ring` counter-clockwise (`ccw`) or clockwise, from the same first point.
fn wound(ring: &[P2], ccw: bool) -> Vec<P2> {
    match ring.split_first() {
        Some((first, rest)) if (geom::area(ring) > 0.0) != ccw => std::iter::once(*first).chain(rest.iter().rev().copied()).collect(),
        _ => ring.to_vec(),
    }
}

fn cap(mut problems: Vec<Problem>) -> Vec<Problem> {
    problems.truncate(MAX_PROBLEMS);
    problems
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ring(pts: &[(f64, f64)]) -> Vec<SketchLine> {
        let p: Vec<P2> = pts.iter().map(|&(x, y)| P2(x, y)).collect();
        geom::ring_edges(&p).map(|(a, b)| SketchLine::new(a, b)).collect()
    }

    fn square(x: f64, y: f64, s: f64) -> Vec<SketchLine> {
        ring(&[(x, y), (x + s, y), (x + s, y + s), (x, y + s)])
    }

    fn lines_in(problems: &[Problem]) -> Vec<usize> {
        let mut v: Vec<usize> = problems.iter().flat_map(Problem::lines).collect();
        v.sort_unstable();
        v.dedup();
        v
    }

    #[test]
    fn lines_in_any_order_either_way_round_close_into_one_outline_counter_clockwise() {
        let mut ls = square(0.0, 0.0, 4000.0);
        ls.swap(0, 2);
        ls[1] = SketchLine::new(ls[1].end(), ls[1].start());
        ls[3] = SketchLine::new(ls[3].end(), ls[3].start());
        // Ends a little apart still meet.
        ls[0] = SketchLine::new(ls[0].start() + P2(0.4, 0.0), ls[0].end());
        let r = solve_one(&ls).unwrap();
        assert!(geom::area(&r.outer) > 0.0 && r.holes.is_empty());
        assert!((r.area() - 16e6).abs() < 2000.0, "{}", r.area());
        // Drawn clockwise, it comes out the same way round.
        let cw = ring(&[(0.0, 0.0), (0.0, 3000.0), (3000.0, 3000.0), (3000.0, 0.0)]);
        assert!(geom::area(&solve_one(&cw).unwrap().outer) > 0.0);
    }

    #[test]
    fn a_loop_inside_is_a_hole_and_one_inside_that_an_island() {
        let mut ls = square(0.0, 0.0, 10_000.0);
        ls.extend(square(2000.0, 2000.0, 6000.0));
        ls.extend(square(4000.0, 4000.0, 2000.0));
        ls.extend(square(20_000.0, 0.0, 1000.0));
        let p = solve(&ls).unwrap();
        assert_eq!(p.regions.len(), 3, "the outline, the island, the one apart");
        assert_eq!(p.regions[0].holes.len(), 1);
        assert!(geom::area(&p.regions[0].holes[0]) < 0.0, "holes are clockwise");
        assert!(p.regions[1].holes.is_empty() && p.regions[2].holes.is_empty());
        assert!((p.area() - (100e6 - 36e6 + 4e6 + 1e6)).abs() < 1.0);
        assert_eq!(solve_one(&ls), Err(vec![Problem::Separate { count: 3 }]));
    }

    #[test]
    fn an_open_loop_names_its_loose_ends() {
        let mut ls = square(0.0, 0.0, 4000.0);
        ls.pop();
        let e = solve(&ls).unwrap_err();
        assert_eq!(e, vec![Problem::Open { line: 0, at: P2(0.0, 0.0) }, Problem::Open { line: 2, at: P2(0.0, 4000.0) }]);
        assert!(e[0].to_string().contains("open"), "{}", e[0]);
        // A gap wider than TOL is open too.
        let mut ls = square(0.0, 0.0, 4000.0);
        ls[3] = SketchLine::new(ls[3].start(), ls[3].end() + P2(0.0, 5.0));
        assert!(matches!(solve(&ls).unwrap_err()[..], [Problem::Open { line: 0, .. }, Problem::Open { line: 3, .. }]));
    }

    #[test]
    fn crossing_lines_and_a_line_ending_on_another_intersect() {
        // A bow tie.
        let bow = ring(&[(0.0, 0.0), (4000.0, 4000.0), (4000.0, 0.0), (0.0, 4000.0)]);
        let e = solve(&bow).unwrap_err();
        assert_eq!(e, vec![Problem::Cross { lines: [0, 2], at: P2(2000.0, 2000.0) }]);
        // A line ending part way along another: a T, which also leaves three lines at no point but an open end.
        let mut ls = square(0.0, 0.0, 4000.0);
        ls.push([P2(2000.0, 0.0), P2(2000.0, 2000.0)].into());
        let e = solve(&ls).unwrap_err();
        assert!(e.contains(&Problem::Cross { lines: [0, 4], at: P2(2000.0, 0.0) }), "{e:?}");
        assert!(e.contains(&Problem::Open { line: 4, at: P2(2000.0, 2000.0) }), "{e:?}");
        // Two loops crossing each other.
        let mut ls = square(0.0, 0.0, 4000.0);
        ls.extend(square(2000.0, 2000.0, 4000.0));
        let e = solve(&ls).unwrap_err();
        assert_eq!(lines_in(&e), vec![1, 2, 4, 7]);
        assert!(e.iter().all(|p| matches!(p, Problem::Cross { .. })));
    }

    #[test]
    fn lines_along_each_other_overlap_and_a_line_drawn_twice_does_too() {
        let mut ls = square(0.0, 0.0, 4000.0);
        ls.push([P2(1000.0, 0.0), P2(3000.0, 0.0)].into());
        assert!(solve(&ls).unwrap_err().contains(&Problem::Overlap { lines: [0, 4] }));
        let mut ls = square(0.0, 0.0, 4000.0);
        ls.push(SketchLine::new(ls[2].end(), ls[2].start()));
        assert!(solve(&ls).unwrap_err().contains(&Problem::Overlap { lines: [2, 4] }));
        // A straight edge in two lines, end to end, is fine.
        let ls = ring(&[(0.0, 0.0), (2000.0, 0.0), (4000.0, 0.0), (4000.0, 4000.0), (0.0, 4000.0)]);
        assert!(solve_one(&ls).is_ok());
    }

    #[test]
    fn loops_touching_at_a_corner_branch_there() {
        let mut ls = square(0.0, 0.0, 4000.0);
        ls.extend(square(4000.0, 4000.0, 4000.0));
        let e = solve(&ls).unwrap_err();
        assert_eq!(e, vec![Problem::Branch { lines: vec![1, 2, 4, 7], at: P2(4000.0, 4000.0) }]);
        assert!(e[0].to_string().contains("line 1, line 2, line 4 and line 7"), "{}", e[0]);
    }

    #[test]
    fn lines_too_short_or_out_of_range_are_named_before_anything_else() {
        let mut ls = square(0.0, 0.0, 4000.0);
        ls.push([P2(9000.0, 0.0), P2(9000.5, 0.0)].into());
        ls.push([P2(f64::NAN, 0.0), P2(1.0, 1.0)].into());
        ls.push([P2(0.0, 0.0), P2(2e9, 0.0)].into());
        assert_eq!(
            solve(&ls).unwrap_err(),
            vec![Problem::TooShort { line: 4 }, Problem::OutOfRange { line: 5 }, Problem::OutOfRange { line: 6 }]
        );
        assert_eq!(solve(&[]).unwrap_err(), vec![Problem::Empty]);
        let many = vec![SketchLine::new(P2(0.0, 0.0), P2(10.0, 0.0)); MAX_LINES + 1];
        assert_eq!(solve(&many).unwrap_err(), vec![Problem::TooMany { count: MAX_LINES + 1, what: "lines" }]);
    }

    #[test]
    fn a_thin_loop_lies_along_itself_and_a_loop_too_small_to_have_an_area_says_so() {
        // Three points almost in a line: the loop is closed but flat, its sides along each other.
        let ls = ring(&[(0.0, 0.0), (4000.0, 0.5), (8000.0, 0.0)]);
        let e = solve(&ls).unwrap_err();
        assert!(e.iter().all(|p| matches!(p, Problem::Overlap { .. })) && !e.is_empty(), "{e:?}");
        // Lines long enough, ends apart, none along another, but a triangle of 1.5 mm sides is under a square millimetre.
        let h = 1.5 * 3f64.sqrt() / 2.0;
        let ls = ring(&[(0.0, 0.0), (1.5, 0.0), (0.75, h)]);
        assert_eq!(solve(&ls).unwrap_err(), vec![Problem::NoArea { lines: vec![0, 1, 2] }]);
    }

    #[test]
    fn a_line_that_crosses_itself_is_named_once_whatever_its_shape() {
        let poly =
            |points: &[(f64, f64)], closed| Curve::Poly { points: points.iter().map(|&(x, y)| P2(x, y)).collect(), arcs: vec![], closed };
        let mut shapes = vec![
            // Four corners joined bow-tie fashion: the first and third sides cross.
            poly(&[(0.0, 0.0), (4000.0, 4000.0), (4000.0, 0.0), (0.0, 2000.0)], true),
            // Open, coming back across its own start.
            poly(&[(0.0, 0.0), (4000.0, 0.0), (4000.0, 2000.0), (2000.0, -1000.0)], false),
            // A cubic with a loop in it.
            Curve::Spline { control: vec![P2(0.0, 0.0), P2(5000.0, 3000.0), P2(-1000.0, 3000.0), P2(4000.0, 0.0)], knots: None },
            Curve::Fit { points: vec![P2(0.0, 0.0), P2(4000.0, 3000.0), P2(0.0, 3000.0), P2(4000.0, 0.0)], tangents: [None, None] },
        ];
        // An arc crossed by the line after it, in one polyline.
        shapes.push(Curve::Poly {
            points: vec![P2(0.0, 0.0), P2(4000.0, 0.0), P2(2000.0, 3000.0), P2(2000.0, -1000.0)],
            arcs: vec![90.0, 0.0, 0.0],
            closed: false,
        });
        for c in shapes {
            let e = solve(&[c.clone().into()]).unwrap_err();
            let own: Vec<&Problem> = e.iter().filter(|p| matches!(p, Problem::Cross { lines: [0, 0], .. })).collect();
            assert_eq!(own.len(), 1, "{c:?}: {e:?}");
            assert_eq!(own[0].lines(), vec![0]);
            assert!(own[0].to_string().contains("line 0 crosses itself at"), "{}", own[0]);
        }
        // A polyline that only turns, never touching itself, is no problem to the line.
        let spiral = poly(&[(0.0, 0.0), (4000.0, 0.0), (4000.0, 4000.0), (1000.0, 4000.0), (1000.0, 1000.0)], false);
        assert!(solve(&[spiral.into()]).unwrap_err().iter().all(|p| !matches!(p, Problem::Cross { .. })));
        // Doubling back along itself is a line along itself.
        let back = poly(&[(0.0, 0.0), (4000.0, 0.0), (1000.0, 0.0)], false);
        assert!(solve(&[back.into()]).unwrap_err().contains(&Problem::Overlap { lines: [0, 0] }));
    }

    #[test]
    fn ends_meet_within_a_millimetre_and_no_further_whichever_cell_they_fall_in() {
        // The last side ends exactly 1 mm from where the first begins: one corner. Another 0.01 mm and the loop is open.
        for (gap, meets) in [(1.0, true), (0.999, true), (1.01, false)] {
            let mut ls = square(0.0, 0.0, 4000.0);
            ls[3] = SketchLine::new(ls[3].start(), P2(-gap, 0.0));
            assert_eq!(solve(&ls).is_ok(), meets, "{gap} mm apart");
        }
        // Across a corner of the grid the weld looks in: 0.28 mm apart, in cells diagonal to each other.
        let mut ls = square(0.0, 0.0, 4000.0);
        ls[0] = SketchLine::new(P2(0.9, 0.9), ls[0].end());
        ls[3] = SketchLine::new(ls[3].start(), P2(1.1, 1.1));
        assert!(solve(&ls).is_ok());
    }

    #[test]
    fn welding_finds_what_a_scan_of_every_point_finds() {
        let mut seed = 11u64;
        let mut next = |range: f64| {
            seed = crate::model::mix(seed);
            (seed % 100_000) as f64 / 100_000.0 * range
        };
        // Clustered so that many are within a millimetre of several, across cells.
        let given: Vec<P2> = (0..3000).map(|_| P2(next(40.0) - 20.0, next(40.0) - 20.0)).collect();
        let (mut welder, mut scanned) = (Welder::default(), Vec::<P2>::new());
        for p in given {
            let by_scan = scanned.iter().position(|q| q.dist(p) <= TOL).unwrap_or_else(|| {
                scanned.push(p);
                scanned.len() - 1
            });
            assert_eq!(welder.weld(p), by_scan, "{p:?}");
        }
        assert_eq!(welder.points, scanned);
    }

    #[test]
    fn the_box_tree_finds_the_pairs_and_the_points_a_scan_finds() {
        let mut seed = 5u64;
        let mut next = |range: f64| {
            seed = crate::model::mix(seed);
            (seed % 10_000) as f64 / 10_000.0 * range
        };
        for n in [0, 1, 7, 9, 300, 1500] {
            let boxes: Vec<[f64; 4]> = (0..n)
                .map(|_| {
                    let (x, y) = (next(1000.0), next(1000.0));
                    [x, y, x + next(120.0), y + next(120.0)]
                })
                .collect();
            let mut want: Vec<(usize, usize)> = Vec::new();
            for i in 0..n {
                for j in i + 1..n {
                    if overlap(&boxes[i], &boxes[j]) {
                        want.push((i, j));
                    }
                }
            }
            let tree = Boxes::new(boxes.clone());
            let mut got: Vec<(usize, usize)> = Vec::new();
            let mut work = 0;
            assert_eq!(
                tree.pairs(&mut work, |i, j| {
                    got.push((i.min(j), i.max(j)));
                    true
                }),
                Ok(true)
            );
            got.sort_unstable();
            assert_eq!(got, want, "{n} boxes: each pair once");
            for _ in 0..50 {
                let p = P2(next(1100.0), next(1100.0));
                let mut hit: Vec<usize> = Vec::new();
                tree.at(p, |i| {
                    hit.push(i);
                    true
                });
                hit.sort_unstable();
                let want: Vec<usize> =
                    (0..n).filter(|&i| boxes[i][0] <= p.0 && p.0 <= boxes[i][2] && boxes[i][1] <= p.1 && p.1 <= boxes[i][3]).collect();
                assert_eq!(hit, want, "{n} boxes at {p:?}");
            }
        }
    }

    #[test]
    fn a_bend_under_a_millimetre_is_said_where_it_is_not_as_a_branch() {
        // A jog of half a millimetre in a polyline, drawn on purpose or by a slip of the mouse.
        let jog = Curve::Poly {
            points: vec![P2(0.0, 0.0), P2(4000.0, 0.0), P2(4000.0, 0.5), P2(4000.0, 3000.0), P2(0.0, 3000.0)],
            arcs: vec![],
            closed: true,
        };
        let e = solve(&[jog.into()]).unwrap_err();
        assert_eq!(e.len(), 1, "{e:?}");
        let said = e[0].to_string();
        assert!(said.contains("less than 1 mm apart, at [4000, 0]") && !said.contains("meet"), "{said}");
    }

    #[test]
    fn an_outline_and_its_holes_come_back_as_lines_that_solve_to_them() {
        let outline = vec![P2(0.0, 0.0), P2(6000.0, 0.0), P2(6000.0, 6000.0), P2(0.0, 6000.0)];
        let hole = vec![P2(1000.0, 1000.0), P2(1000.0, 2000.0), P2(2000.0, 2000.0), P2(2000.0, 1000.0)];
        let ls = lines_of(&outline, std::slice::from_ref(&hole));
        assert_eq!(ls.len(), 8);
        let r = solve_one(&ls).unwrap();
        assert_eq!((r.outer, r.holes), (outline, vec![hole]));
    }

    #[test]
    fn many_problems_are_capped() {
        // A comb: every tooth crosses the spine.
        let mut ls = vec![SketchLine::new(P2(0.0, 0.0), P2(1e6, 0.0))];
        ls.extend((1..500).map(|i| SketchLine::new(P2(i as f64 * 1000.0, -500.0), P2(i as f64 * 1000.0, 500.0))));
        let e = solve(&ls).unwrap_err();
        assert_eq!(e.len(), MAX_PROBLEMS);
        let name = |i: usize| format!("line {i}");
        assert!(say(&e, &name).contains("more"), "{}", say(&e, &name));
    }

    #[test]
    fn random_lines_never_panic_and_what_solves_is_sound() {
        let mut seed = 7u64;
        let mut next = || {
            seed = crate::model::mix(seed);
            (seed % 9000) as f64
        };
        for _ in 0..400 {
            let n = 1 + (next() as usize) % 9;
            let ls: Vec<SketchLine> = (0..n).map(|_| SketchLine::new(P2(next(), next()), P2(next(), next()))).collect();
            // Closed as a ring too, to make loops more often.
            let pts: Vec<P2> = ls.iter().map(|l| l.start()).collect();
            for sk in [ls, geom::ring_edges(&pts).map(|(a, b)| SketchLine::new(a, b)).collect()] {
                if let Ok(p) = solve(&sk) {
                    for r in &p.regions {
                        assert!(geom::area(&r.outer) > 0.0 && r.holes.iter().all(|h| geom::area(h) < 0.0));
                    }
                    assert!(p.area() > 0.0);
                }
            }
        }
    }

    #[test]
    fn an_arc_closes_a_loop_and_is_one_line_however_many_chords_draw_it() {
        // A half disc: its diameter, then the arc back round through the top.
        let ls = vec![SketchLine::new(P2(0.0, 0.0), P2(4000.0, 0.0)), SketchLine::arc(P2(4000.0, 0.0), P2(0.0, 0.0), P2(2000.0, 2000.0))];
        let r = solve_one(&ls).unwrap();
        let half = std::f64::consts::PI * 2000.0 * 2000.0 / 2.0;
        assert!((r.area() - half).abs() / half < 0.01, "{} vs {half}", r.area());
        assert!(r.outer.len() > 20, "drawn by its chords");
        assert!((ls[1].middle().dist(P2(2000.0, 2000.0))) < 1.0, "{:?}", ls[1].middle());
        // A line across it hits the arc: one problem, naming the arc once.
        let mut crossed = ls.clone();
        crossed.push(SketchLine::new(P2(1000.0, 1000.0), P2(1000.0, 3000.0)));
        let e = solve(&crossed).unwrap_err();
        assert_eq!(e[0], Problem::Cross { lines: [1, 2], at: e[0].at().unwrap() }, "{e:?}");
        assert_eq!(e.iter().filter(|p| matches!(p, Problem::Cross { .. })).count(), 1);
        // A point through on the straight line between its ends is a straight line.
        assert_eq!(SketchLine::arc(P2(0.0, 0.0), P2(4000.0, 0.0), P2(2000.0, 0.5)).points().len(), 2);
    }

    #[test]
    fn crossings_come_before_the_open_ends_they_leave_and_lines_are_named_by_id() {
        // Lines overshooting each other at a corner: they cross, and leave ends hanging past it.
        let mut ls = ring(&[(0.0, 0.0), (4000.0, 0.0), (4000.0, 4000.0)]);
        ls.truncate(2);
        ls.push(SketchLine::new(P2(4000.0, 4500.0), P2(-500.0, 4500.0)));
        ls.push(SketchLine::new(P2(0.0, 5000.0), P2(0.0, 0.0)));
        let e = solve(&ls).unwrap_err();
        assert!(matches!(e[0], Problem::Cross { .. }), "{e:?}");
        assert!(e.iter().any(|p| matches!(p, Problem::Open { .. })));
        ls[3].id = 999_812;
        let name = namer(&ls);
        let said = say(&e, &name);
        assert!(said.contains("line 2 and line 999812 intersect at [0, 4500]"), "{said}");
    }

    #[test]
    fn a_kept_sketch_draws_its_outline_from_any_start_either_way_round() {
        let outline = vec![P2(0.0, 0.0), P2(6000.0, 0.0), P2(6000.0, 6000.0), P2(0.0, 6000.0)];
        let ls = lines_of(&outline, &[]);
        assert!(draws(&ls, &outline, &[]));
        let turned: Vec<P2> = outline.iter().rev().copied().collect();
        assert!(draws(&ls, &turned, &[]));
        let moved: Vec<P2> = outline.iter().map(|p| *p + P2(10.0, 0.0)).collect();
        assert!(!draws(&ls, &moved, &[]));
        assert!(!draws(&ls, &outline, std::slice::from_ref(&outline)));
    }
}

/// The sketch through the commands: a dry run, and the outlines of floors, ceilings, roofs and shafts drawn or edited as one.
#[cfg(test)]
mod command_tests {
    use serde_json::{Value, json};

    use crate::engine::Session;
    use crate::model::Element;

    fn run(s: &mut Session, id: &str, p: Value) -> Value {
        s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
    }

    /// A square's lines, `[x, y]` its south-west corner, drawn clockwise.
    fn square(x: f64, y: f64, size: f64) -> Vec<Value> {
        let c = [[x, y], [x, y + size], [x + size, y + size], [x + size, y]];
        (0..4).map(|i| json!([c[i], c[(i + 1) % 4]])).collect()
    }

    fn with_hole() -> Value {
        let mut ls = square(0.0, 0.0, 6000.0);
        ls.extend(square(2000.0, 2000.0, 1000.0));
        Value::Array(ls)
    }

    #[test]
    fn a_sketch_is_solved_without_changing_the_model_and_says_what_is_wrong_line_by_line() {
        let mut s = Session::new();
        let steps = s.history().len();
        let r = run(&mut s, "sketch", json!({"lines": with_hole()}));
        assert_eq!(r["regions"].as_array().map(Vec::len), Some(1));
        assert_eq!(r["regions"][0]["holes"].as_array().map(Vec::len), Some(1));
        assert_eq!(r["area_m2"], 35.0);
        assert_eq!(r["regions"][0]["outline"][1], json!([6000.0, 0.0]), "counter-clockwise whichever way it was drawn");
        assert_eq!(s.history().len(), steps, "no undo step");
        // Open, crossing: every problem, with the lines it's about.
        let mut ls = square(0.0, 0.0, 4000.0);
        ls.pop();
        ls.push(json!([[-1000, 2000], [1000, 2000]]));
        let e = s.execute("sketch", &json!({"lines": ls})).unwrap_err();
        assert!(e.contains("line 0 and line 3 intersect at [0, 2000]"), "{e}");
        assert!(e.contains("line 2 ends at [4000, 0] without meeting another"), "{e}");
        let e = s.execute("sketch", &json!({"lines": [[[0, 0], [1]]]})).unwrap_err();
        assert!(e.contains("`lines` is"), "{e}");
    }

    #[test]
    fn floors_ceilings_roofs_and_shafts_take_their_outline_and_holes_as_a_sketch() {
        let mut s = Session::new();
        let f = run(&mut s, "floor", json!({"sketch": with_hole()}));
        assert_eq!(f["area_m2"], 35.0);
        let Some(Element::Floor(fl)) = s.model.elements.get(f["id"].as_u64().unwrap()).cloned() else { panic!() };
        assert_eq!((fl.boundary.len(), fl.openings.len()), (4, 1));
        assert!(crate::geom::area(&fl.boundary) > 0.0);
        let c = run(&mut s, "ceiling", json!({"sketch": with_hole()}));
        assert_eq!(c["area_m2"], 35.0);
        let r = run(&mut s, "roof", json!({"sketch": with_hole(), "slope": 0}));
        let Some(Element::Roof(rf)) = s.model.elements.get(r["id"].as_u64().unwrap()) else { panic!() };
        assert_eq!(rf.courtyards.len(), 1, "a roof's holes are its courtyards");
        run(&mut s, "shaft", json!({"sketch": square(0.0, 0.0, 1000.0)}));
        let e = s.execute("shaft", &json!({"sketch": with_hole()})).unwrap_err();
        assert!(e.contains("no holes"), "{e}");
        // One outline: two apart, or a sketch with a boundary too, is refused.
        let mut two = square(0.0, 0.0, 1000.0);
        two.extend(square(5000.0, 0.0, 1000.0));
        let e = s.execute("floor", &json!({"sketch": two})).unwrap_err();
        assert!(e.contains("2 separate outlines"), "{e}");
        let e = s.execute("floor", &json!({"sketch": with_hole(), "boundary": [[0, 0], [1000, 0], [0, 1000]]})).unwrap_err();
        assert!(e.contains("`sketch` or `boundary`"), "{e}");
    }

    #[test]
    fn a_boundary_given_as_points_is_judged_as_the_sketch_of_its_edges() {
        let mut s = Session::new();
        let e = s.execute("floor", &json!({"boundary": [[0, 0], [4000, 4000], [4000, 0], [0, 4000]]})).unwrap_err();
        assert!(e.contains("edge 0 and edge 2 intersect at [2000, 2000]"), "{e}");
        let e = s.execute("floor", &json!({"boundary": [[0, 0], [4000, 0], [4000, 4000], [0, 4000]], "openings": [[[1000, 1000], [2000, 2000], [2000, 1000], [1000, 2000]]]}))
            .unwrap_err();
        assert!(e.contains("opening 0") && e.contains("intersect"), "{e}");
        let e = s.execute("roof", &json!({"boundary": [[0, 0], [4000, 4000], [4000, 0], [0, 4000]]})).unwrap_err();
        assert!(e.contains("intersect"), "{e}");
        // Closed back to its first point, or a point twice, it's the same outline.
        let f = run(&mut s, "floor", json!({"boundary": [[0, 0], [4000, 0], [4000, 0], [4000, 4000], [0, 4000], [0, 0]]}));
        let Some(Element::Floor(fl)) = s.model.elements.get(f["id"].as_u64().unwrap()) else { panic!() };
        assert_eq!(fl.boundary.len(), 4);
    }

    #[test]
    fn one_line_crossing_itself_is_refused_as_the_same_points_in_four_lines_are() {
        let mut s = Session::new();
        let bow = json!([[0, 0], [4000, 4000], [4000, 0], [0, 2000]]);
        // A closed polyline is one sketch line: it used to pass, and report the shoelace area, 4 m², where the loop of
        // the two triangles it makes is not one outline at all.
        let one = json!([{"points": bow, "closed": true}]);
        for cmd in ["floor", "ceiling", "shaft", "sketch", "filled_region"] {
            let p = if cmd == "sketch" { json!({"lines": one}) } else { json!({"sketch": one}) };
            let e = s.execute(cmd, &p).unwrap_err();
            assert!(e.contains("line 0 crosses itself at [1333, 1333]"), "{cmd}: {e}");
        }
        let four = Value::Array((0..4).map(|i| json!([bow[i], bow[(i + 1) % 4]])).collect());
        let e = s.execute("floor", &json!({"sketch": four})).unwrap_err();
        assert!(e.contains("line 0 and line 2 intersect at [1333, 1333]"), "{e}");
        assert!(s.model.elements.is_empty() || s.model.elements.values().all(|e| !matches!(e, Element::Floor(_))));
    }

    #[test]
    fn edit_boundary_redraws_an_outline_in_one_step_or_leaves_it_as_it_was() {
        let mut s = Session::new();
        let f = run(&mut s, "floor", json!({"boundary": [[0, 0], [4000, 0], [4000, 4000], [0, 4000]]}))["id"].clone();
        // What it is now, as lines to edit.
        let now = run(&mut s, "edit_boundary", json!({"id": f}));
        assert_eq!(now["sketch"].as_array().map(Vec::len), Some(4));
        let r = run(&mut s, "edit_boundary", json!({"id": f, "sketch": with_hole()}));
        assert_eq!(r["area_m2"], 35.0);
        let Some(Element::Floor(fl)) = s.model.elements.get(f.as_u64().unwrap()) else { panic!() };
        assert_eq!(fl.openings.len(), 1);
        assert_eq!(s.history().last(), Some(&"Edit Boundary"));
        let mut open = square(0.0, 0.0, 4000.0);
        open.pop();
        let before = s.model.clone();
        let e = s.execute("edit_boundary", &json!({"id": f, "sketch": open})).unwrap_err();
        assert!(e.contains("open"), "{e}");
        assert_eq!(s.model, before);
        run(&mut s, "undo", json!({}));
        let Some(Element::Floor(fl)) = s.model.elements.get(f.as_u64().unwrap()) else { panic!() };
        assert!(fl.openings.is_empty());
        // A wall has no boundary.
        let w = run(&mut s, "wall", json!({"points": [[0, 0], [1000, 0]]}))["ids"][0].clone();
        let e = s.execute("edit_boundary", &json!({"id": w, "sketch": with_hole()})).unwrap_err();
        assert!(e.contains("floor, a ceiling, a roof's footprint, a shaft or a filled region"), "{e}");
    }

    #[test]
    fn a_roofs_gable_edges_stay_gables_where_the_edited_footprint_keeps_them() {
        let mut s = Session::new();
        let r = run(&mut s, "roof", json!({"boundary": [[0, 0], [8000, 0], [8000, 6000], [0, 6000]], "gables": [1, 3], "slope": 30}))["id"]
            .clone();
        // Its south edge moved down: the east and west edges are new lines, the north edge the same.
        let lines = json!([[[0, -1000], [8000, -1000]], [[8000, -1000], [8000, 6000]], [[8000, 6000], [0, 6000]], [[0, 6000], [0, -1000]]]);
        run(&mut s, "edit_boundary", json!({"id": r, "sketch": lines}));
        let Some(Element::Roof(rf)) = s.model.elements.get(r.as_u64().unwrap()) else { panic!() };
        assert!(rf.gables.is_empty(), "{:?}", rf.gables);
        // Back, and the gables on the east and west ends again, then the north edge in two lines.
        run(&mut s, "undo", json!({}));
        let lines = json!([
            [[0, 0], [8000, 0]],
            [[8000, 0], [8000, 6000]],
            [[8000, 6000], [4000, 6000]],
            [[4000, 6000], [0, 6000]],
            [[0, 6000], [0, 0]]
        ]);
        run(&mut s, "edit_boundary", json!({"id": r, "sketch": lines}));
        let Some(Element::Roof(rf)) = s.model.elements.get(r.as_u64().unwrap()) else { panic!() };
        assert_eq!(rf.gables, vec![1, 4]);
        // An extrusion roof is edited by its profile.
        let x = run(
            &mut s,
            "roof",
            json!({"extrusion": {"line": [[0, 0], [4000, 0]], "profile": [[0, 0], [2000, 1000], [4000, 0]], "to": 3000}}),
        )["id"]
            .clone();
        let e = s.execute("edit_boundary", &json!({"id": x, "sketch": with_hole()})).unwrap_err();
        assert!(e.contains("extrusion"), "{e}");
    }

    /// A floor's sketch lines, as `elements` gives them.
    fn lines(s: &mut Session, id: &Value) -> Vec<Value> {
        let r = run(s, "elements", json!({"category": "floor"}));
        let found = r["elements"].as_array().and_then(|es| es.iter().find(|e| e["id"] == *id).cloned()).unwrap_or_default();
        found["sketch"].as_array().cloned().unwrap_or_default()
    }

    #[test]
    fn each_sketch_line_has_an_id_and_a_guid_of_its_own_kept_through_edits_and_moves() {
        let mut s = Session::new();
        let f = run(&mut s, "floor", json!({"sketch": square(0.0, 0.0, 6000.0)}))["id"].clone();
        let kept = lines(&mut s, &f);
        assert_eq!(kept.len(), 4);
        let ids: Vec<u64> = kept.iter().filter_map(|l| l["id"].as_u64()).collect();
        assert_eq!(ids.len(), 4, "{kept:?}");
        assert!(ids.iter().all(|&i| (100_000..=999_999).contains(&i)), "{ids:?}");
        assert!(kept.iter().all(|l| l["guid"].is_string()));
        // Edit Boundary gives them back; three sent again by id keep theirs, the fourth redrawn as two is new.
        let now = run(&mut s, "edit_boundary", json!({"id": f}))["sketch"].clone();
        assert_eq!(now.as_array().map(Vec::len), Some(4));
        let mut again: Vec<Value> =
            now.as_array().unwrap()[..3].iter().map(|l| json!({"id": l["id"], "start": l["start"], "end": l["end"]})).collect();
        let last = &now[3];
        let mid = json!([3000.0, 0.0]);
        again.push(json!([last["start"], mid]));
        again.push(json!([mid, last["end"]]));
        run(&mut s, "edit_boundary", json!({"id": f, "sketch": again}));
        let after = lines(&mut s, &f);
        let after_ids: Vec<u64> = after.iter().filter_map(|l| l["id"].as_u64()).collect();
        assert_eq!(after_ids[..3], ids[..3]);
        assert!(after_ids[3..].iter().all(|i| !ids.contains(i)) && after_ids.len() == 5, "{after_ids:?}");
        // An id that isn't one of its lines is refused.
        let e = s.execute("edit_boundary", &json!({"id": f, "sketch": [{"id": 123_456, "start": [0, 0], "end": [1, 1]}]})).unwrap_err();
        assert!(e.contains("123456 isn't a line of this sketch"), "{e}");
        // Moved, the sketch goes with it and keeps its ids; copied, the copy's lines are new.
        run(&mut s, "move", json!({"ids": [f], "by": [1000, 0]}));
        let moved = lines(&mut s, &f);
        assert_eq!(moved[0]["id"], after[0]["id"]);
        assert_eq!(moved[0]["start"][0].as_f64(), after[0]["start"][0].as_f64().map(|x| x + 1000.0));
        let copy = run(&mut s, "copy", json!({"ids": [f], "by": [0, 10_000]}))["ids"][0].clone();
        let copied: Vec<u64> = lines(&mut s, &copy).iter().filter_map(|l| l["id"].as_u64()).collect();
        assert_eq!(copied.len(), 5);
        assert!(copied.iter().all(|i| !after_ids.contains(i)), "{copied:?}");
        // A floor made over the walls keeps none until its boundary is edited; its lines then come as new ones.
        run(&mut s, "wall", json!({"points": [[20000, 0], [26000, 0], [26000, 6000], [20000, 6000]], "closed": true}));
        let w = run(&mut s, "floor", json!({}))["id"].clone();
        assert!(lines(&mut s, &w).is_empty());
        let fresh = run(&mut s, "edit_boundary", json!({"id": w}))["sketch"].clone();
        assert!(fresh.as_array().unwrap().iter().all(|l| l.get("id").is_none()));
    }

    #[test]
    fn a_sketch_lines_id_is_not_given_again_while_the_project_is_open_after_the_line_has_gone() {
        let mut s = Session::new();
        let f = run(&mut s, "floor", json!({"sketch": square(0.0, 0.0, 4000.0)}))["id"].clone();
        let first = lines(&mut s, &f);
        let ids: Vec<u64> = first.iter().filter_map(|l| l["id"].as_u64()).collect();
        // Down to a triangle on the first three lines' ids: the fourth's goes.
        let tri = [([0, 0], [4000, 0]), ([4000, 0], [4000, 4000]), ([4000, 4000], [0, 0])];
        let kept: Vec<Value> = tri.iter().zip(&ids).map(|((a, b), id)| json!({"id": id, "start": a, "end": b})).collect();
        run(&mut s, "edit_boundary", json!({"id": f, "sketch": kept}));
        let now: Vec<u64> = lines(&mut s, &f).iter().filter_map(|l| l["id"].as_u64()).collect();
        assert_eq!(now, ids[..3], "the three keep theirs");
        // A line added next time: its id is nobody's before, the fourth's included.
        let mut more: Vec<Value> = kept.clone();
        more[2] = json!({"id": ids[2], "start": [2000, 2000], "end": [0, 0]});
        more.push(json!({"start": [4000, 4000], "end": [2000, 2000]}));
        run(&mut s, "edit_boundary", json!({"id": f, "sketch": more}));
        let after: Vec<u64> = lines(&mut s, &f).iter().filter_map(|l| l["id"].as_u64()).collect();
        assert_eq!(after.len(), 4);
        assert!(!ids[3..].contains(&after[3]) && after[3] < ids[3], "{after:?} against {ids:?}");
        // A copy's lines are new ones too, lower still.
        let copy = run(&mut s, "copy", json!({"ids": [f], "by": [0, 9000]}))["ids"][0].clone();
        let copied: Vec<u64> = lines(&mut s, &copy).iter().filter_map(|l| l["id"].as_u64()).collect();
        assert!(copied.iter().all(|c| *c < after[3] && !ids.contains(c)), "{copied:?}");
    }

    #[test]
    fn an_arc_sketched_rounds_the_floor() {
        let mut s = Session::new();
        let half = json!([[[0, 0], [4000, 0]], [[4000, 0], [0, 0], [2000, 2000]]]);
        let f = run(&mut s, "floor", json!({"sketch": half}));
        let a = f["area_m2"].as_f64().unwrap();
        assert!((a - std::f64::consts::TAU).abs() < 0.1, "{a}");
        // Kept by how far it turns, counter-clockwise as a wall's arc: west from (4000, 0), bulging north to its right.
        let turn = lines(&mut s, &f["id"])[1]["arc"].as_f64().unwrap_or_default();
        assert!((turn - 180.0).abs() < 1e-6, "{turn}");
    }
}

/// Hostile and dense sketches: each finishes well under a second.
#[cfg(test)]
mod hostile_tests {
    use std::time::Instant;

    use serde_json::{Value, json};

    use crate::engine::Session;

    /// Runs `id` and says how long it took, seconds.
    fn timed(s: &mut Session, id: &str, p: &Value) -> (Result<Value, String>, f64) {
        let t = Instant::now();
        let r = s.execute(id, p);
        let took = t.elapsed().as_secs_f64();
        eprintln!("{id}: {took:.3} s");
        (r, took)
    }

    /// `n` concentric circles from radius `r0`, `step` mm apart, as sketch lines.
    fn circles(n: usize, r0: f64, step: f64) -> Value {
        Value::Array((0..n).map(|k| json!({"centre": [0, 0], "radius": r0 + step * k as f64})).collect())
    }

    #[test]
    fn ten_thousand_parallel_lines_are_named_open_at_once() {
        let lines: Vec<Value> = (0..10_000).map(|i| json!([[0, i * 5], [1000, i * 5]])).collect();
        let (r, took) = timed(&mut Session::new(), "sketch", &json!({"lines": lines}));
        assert!(r.unwrap_err().contains("open"));
        assert!(took < 1.0, "{took} s");
    }

    #[test]
    fn five_rings_of_ten_thousand_points_nest_at_once() {
        let ring = |r: f64| -> Value {
            let pts: Vec<Value> = (0..10_000)
                .map(|i| {
                    let a = std::f64::consts::TAU * i as f64 / 10_000.0;
                    json!([(r * a.cos()).round(), (r * a.sin()).round()])
                })
                .collect();
            json!({"points": pts, "closed": true})
        };
        let lines: Vec<Value> = (0..5).map(|k| ring(1.0e6 + 2.0e4 * k as f64)).collect();
        let (r, took) = timed(&mut Session::new(), "sketch", &json!({"lines": lines}));
        assert!(r.is_ok(), "{r:?}");
        assert!(took < 1.0, "{took} s");
    }

    #[test]
    fn concentric_circles_at_the_cap_solve_at_once_and_past_it_are_refused_at_once() {
        let mut s = Session::new();
        let (r, took) = timed(&mut s, "sketch", &json!({"lines": circles(10, 1.0e6, 1000.0)}));
        assert_eq!(r.map(|v| v["regions"].as_array().map(Vec::len)), Ok(Some(5)));
        assert!(took < 1.0, "{took} s");
        let (r, took) = timed(&mut s, "sketch", &json!({"lines": circles(40, 1.0e6, 1000.0)}));
        assert!(r.is_ok() || r.as_ref().is_err_and(|e| e.contains("pieces")), "{r:?}");
        assert!(took < 1.0, "{took} s");
        let (r, took) = timed(&mut s, "sketch", &json!({"lines": circles(100, 1.0e6, 1000.0)}));
        assert!(r.unwrap_err().contains("pieces"));
        assert!(took < 1.0, "{took} s");
    }

    #[test]
    fn a_hundred_wide_arcs_are_solved_or_refused_at_once() {
        let arcs: Vec<Value> = (0..100).map(|k| json!([[0, k], [2.0e6, k], [1.0e6, 1.0e6 + k as f64]])).collect();
        let (r, took) = timed(&mut Session::new(), "sketch", &json!({"lines": arcs}));
        assert!(r.is_err());
        assert!(took < 1.0, "{took} s");
    }

    #[test]
    fn a_floor_of_ten_thousand_corners_stays_quick() {
        let pts: Vec<Value> = (0..10_000)
            .map(|i| {
                let a = std::f64::consts::TAU * i as f64 / 10_000.0;
                json!([(1.0e6 * a.cos()).round(), (1.0e6 * a.sin()).round()])
            })
            .collect();
        let (r, took) = timed(&mut Session::new(), "floor", &json!({"boundary": pts}));
        assert!(r.is_ok(), "{r:?}");
        assert!(took < 1.0, "{took} s");
    }

    #[test]
    fn ten_thousand_parallel_diagonals_are_refused_not_waited_for() {
        // Every box holds every other: fifty million pairs to look at, none of them crossing.
        let lines: Vec<Value> = (0..10_000).map(|i| json!([[0, i * 100], [1.0e6, 1.0e6 + i as f64 * 100.0]])).collect();
        let (r, took) = timed(&mut Session::new(), "sketch", &json!({"lines": lines}));
        let e = r.unwrap_err();
        assert!(e.contains("too close together"), "{e}");
        assert!(took < 1.0, "{took} s");
    }

    #[test]
    fn a_sketch_kept_with_too_many_pieces_is_refused_by_the_model_as_the_command_refuses_it() {
        let mut s = Session::new();
        let f = s.execute("floor", &json!({"boundary": [[0, 0], [4000, 0], [4000, 4000], [0, 4000]]})).unwrap()["id"].clone();
        let (r, took) = timed(&mut s, "modify", &json!({"id": f, "sketch": circles(100, 1.0e6, 1000.0)}));
        assert!(r.unwrap_err().contains("straight pieces"));
        assert!(took < 1.0, "{took} s");
    }

    #[test]
    fn twenty_circles_in_a_filled_region_solve_at_once() {
        let (r, took) = timed(&mut Session::new(), "filled_region", &json!({"sketch": circles(20, 1.0e6, 1000.0)}));
        assert!(r.is_ok(), "{r:?}");
        assert!(took < 1.0, "{took} s");
    }
}
