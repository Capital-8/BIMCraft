# Views: scale, detail and style, and visibility and graphics

Moved from the root `CLAUDE.md`, word for word: the root keeps the rules and the map, and this file is read when the area is touched.

A view's scale, detail level and visual style are in its `Graphics` (`scale`, `detail`, `style`; the `scale`, `detail` and `visual_style`
commands, which the view control bar's menus, Properties' View Scale, Scale Value 1:, Detail Level and Visual Style, and WF, HL and SD run). A
view template controls them as groups of their own (`Group::Scale`, `Detail`, `Style`, in `Graphics::resolved`, `check_followed` and the three
commands, which refuse what a template controls); `render::scale_of`, `detail_of` and `Look::with` read the value in force, never the view's own, and
the bar and Properties show it greyed out, the template named on hover (`controlled_note`). A scale is the n of 1 : n (`graphics::SCALES`);
`render::annotation` turns it and the zoom into `Drawing::k`, the factor annotation is drawn at (`Drawing::px`, `Drawing::grow`: annotation is
laid out one pixel to the pixel and grown about its anchor), 1 for a view with no scale, which keeps annotation the size it has on screen,
and 1 on a sheet (`draw_at`, `section_at`), where the viewport's scale already sets paper size. A viewport placed with no scale takes the
view's. `Drawing::detail` is the level the plan or the section is drawn at: coarse leaves out a compound element's layers and an opening's
symbols, medium its patterns (`cut_layer`, a floor's `surface`), fine draws as before. In the 3D view `Look::style` picks how
`Scene::picture` draws: shaded as ever, hidden line as a section's picture is (paper faces, edges), wireframe by `Scene::edges` (where
surfaces meet at an angle or end, found once) drawn through the faces; a pixel within two of a line picks its element.

A view's graphics (`graphics.rs`; `Level`, `Section` and `View3D` each hold `graphics`) hide and override by category, by view
filter (`Model::view_filters`: categories and a schedule filter's rules, `schedule::picks`) and by element: an element's own beats
the filters' (the first applied prevailing), which beat the category's, and hiding by any of them hides. `Look::of` resolves a
view to what it hides and how it paints the rest; the drawing code asks `look.shows(id)` wherever it loops over elements (plans,
sections, `cut_outlines`, `seen_below`, `view3d::Scene::picture`) and `look.dress` recolours the finished `Drawing` by owner and
by `Part` (`Drawing::part` marks what is drawn as projection, cut, lettering or hatching (and the lines between layers), so a
new drawing of what a view cuts sets `Part::Cut`; a hidden door's opening is drawn owned by none, the wall's; the cut walls' union fill has no owner and takes its colour at the draw site, `cut_unions`). A view template
(`Model::view_templates`) controls groups of the views that follow it (categories, filters, a plan's view range through
`Model::view_range_of`, a phase with its phase filter, design options, scale, detail level, visual style), and they can't change those
(`graphics`, `scale`, `detail`, `visual_style` and `modify` refuse). Snaps, Tag All, the desktop's Select All
Instances and the framing of sections and 3D views take what `Look` shows too. A level's ceiling plan is its plan's view;
transparency shows in 3D only.

The 3D view can be drawn on the graphics card (File > Settings > Performance; off until asked for: `BimApp::card`, `settings.json`'s
`gpu_3d`). `view3d::gpu::Mesh` is the triangles a view shows as f32 vertices relative to the middle of what is drawn (millimetres far
from the origin would overflow f32), each with its colour, normal, face normal, plane, element and texture; the textures' mip levels as
words (`Skin`; `Mesh::lookups` gives the level a zoom looks up, as `Sampler::level_for` does); and a bounding volume hierarchy built on
the first pick. Surfaces seen through come after the opaque ones (`Mesh::clear_from`), each element's transparency in its `Owner`.
`Mesh::new` refuses, with a reason, what the card doesn't draw (hidden line, wireframe, more than `MAX_TRIS`), and the processor draws those as before, and sections and sheets always. `Mesh::pick` is `Image::pick` without a picture: the
same projection and barycentric test for triangles (the nearest seen through where it is nearer than the nearest opaque one), and for levels the same `LevelMarks` that `Scene::picture` draws (`level_marks`: one
definition of a level's lines and head). The desktop's `gpu.rs` and `gpu.wgsl` draw it in passes into a texture egui shows like the
processor's picture: the opaque surfaces (lit as `picture` lights them, textured as `Sampler::at` looks up), those seen through (the
nearest in front of the opaque depth, in targets of their own), then an edge wherever the opaque plane changes between a pixel and the one
to its right or below, else the surface tinted halfway where selected, with what is seen through blended over it, and the levels' lines
where nothing opaque nearer hides them. What changes between pictures is the camera and the lines; the owners' tints and the textures'
lookups are written again only when they change. A change to how
the processor shades, draws an edge or marks a level goes in the shader too: `the_card_draws_what_the_processor_does` and the core's
`picking_the_mesh_finds_what_the_cpu_picture_shows` compare them (and, seen through,
`the_card_draws_what_the_view_sees_through_as_the_processor_does` and `picking_through_what_the_view_sees_through_finds_what_the_cpu_picture_shows`), and skip on a machine with no graphics adapter. The card is for Vulkan, Metal and DirectX 12: on OpenGL wgpu panics in the shader compiler for what the shaders read (a depth texture; buffers in a vertex shader), so `Gpu::new` refuses it, and puts whatever else a compiler stops at under `catch_unwind`.
The card is made the first time it is wanted (`Card::ready`: compiling its shaders is not a start's cost), and one that can't be made says why on the Performance page.
A device error is caught
in the card's own error scopes, never left to wgpu's handler (which ends the program), and hands the view to the processor for good.

A level's own plan is the level (its `view_range`, `crop` and `graphics` are the level's); every other plan of it is an `Element::PlanView`
(Revit's Duplicate View: `duplicate_view`, which only plans take). A plan has a key: the level's id, or a plan view's. `Model::plan_level` says
which level a key shows, and everything that draws or measures the model takes that level (`draw_plan`, `plan_shows`, the canvas's tools), while
what is the view's own takes the key: its graphics (`graphics_of`), view range (`view_range_of`, which a template may control), crop (`crop_of`)
and annotation. Annotation says which view it is in: a note or a spot elevation by `view`, a dimension or a tag by an optional `view` beside its
`level` or its host (none: the level's own plan), and `Model::annotation_in` asks whether a plan draws it. A command that takes a `level` takes a
plan view too (`plan_param`): what is built stands on its level, what is dimensioned or tagged is in the view. A dependent view (`parent`) follows
its parent's view range and graphics (`plan_source`; `graphics`, `scale`, `detail` and `visual_style` refuse it, naming the parent), has a crop
of its own and draws its parent's annotation as well as its own. Deleting a view deletes what is annotated in it and its dependents; a level
takes its plan views with it. A project that has none writes the same file as before. A plan's own name, lettered at its bottom left, is in the drawings that make an image (`plan`, `ceiling_plan`: `titled`), not in the window's (`draw`, `draw_ceiling`) or a sheet's (`draw_at`), where the tab and the viewport's title say it. The desktop: View > Duplicate View, DP, and the Project
Browser's right-click, which also has Delete for a plan view; `Sheet::Plan` and `Sheet::Ceiling` hold the key.

## Linked models

A `Link` element (`link.rs`; commands `link` and `reload`) is another project shown in this one as a reference. The file holds its name, its
path, where its origin stands (`at`) and a turn (`angle`); what it read is `Model::loaded`, by path, outside the file (`serde(skip)`), filled by
`link`, `reload` and `open` (`relink`, which doesn't follow the links of a linked file). The views draw `link::seen(model)`: the project with
each loaded link's built elements (`built`: not its views, sheets, annotation, rooms, grids or links) moved and turned by `commands::transformed`
and brought in with `insert::insert_as(.., linked: true)` under ids from `LINKED` up (`ids.rs`), so `link::is_linked(id)` tells them; a level of the
link at the height of one of the project's (1 mm) is that level, another comes in named `link: level`. Everything that takes a `&Model` draws and snaps to
them without knowing: `Look::with` makes a linked id halftone, and picking never answers one (`Drawing::pick_all`, `in_box`, `Image::pick`,
`in_box`, `shows` and `Mesh::pick` leave them out). The commands that make an image (`render`, `save` to svg, png or pdf) draw `seen`; the desktop's
`view_model` does, cached by the project's `Arc`. The schedules, `Model::check`, IFC and every command that edits work on the project itself.
To do: shared coordinates, a link's graphics overrides and visibility per view, hosting on a linked wall, unloading.
