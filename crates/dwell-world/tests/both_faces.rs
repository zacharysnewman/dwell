//! The both-faces check (BIFACIAL_WORLD.md §11.2, CLAUDE.md "both faces"): scans the face-sensitive
//! crates' sources and fails when
//!
//! 1. code mirrors by hand outside the face frame (`dwell_world::FaceFrame`): the mirror's sums,
//!    `2 × MIDPLANE_Y`, a comparison with `MIDPLANE_Y` to find a face, a WGSL face sign made with
//!    `select(-1.0, 1.0, …)`, or the wire's face bit (`FACE_B`) read or written outside the
//!    netcode's `face_of_flags` / `face_bit`; a line marked `// face-boundary: <reason>` (on it, or the
//!    line above or below) is one of the documented boundaries and passes;
//! 2. a plain `#[test]` builds a face-dependent fixture (it names face B, the midplane or a mirror)
//!    without running on both faces: it must be a `both_faces!` test, loop over
//!    `FaceFrame::under_test()`, or say why it is not, with `// face-a-only: <reason>`,
//!    `// face-b-only: <reason>` or `// both-faces: <reason>` (a test that compares or crosses the
//!    faces itself) in its body or on the lines above it.
//!
//! The physics' integration tests (`dwell-physics/tests`) are exempt from rule 2: CI runs that
//! whole suite again in face B's mirror image (`DWELL_FACE=b`).

use std::fs;
use std::path::{Path, PathBuf};

/// The face-sensitive sources: every crate whose code depends on which way is up.
const SCANNED: &[&str] = &[
    "dwell-worldgen/src",
    "dwell-worldgen/tests",
    "dwell-world/src",
    "dwell-world/tests",
    "dwell-mesh/src",
    "dwell-lod/src",
    "dwell-physics/src",
    "dwell-physics/tests",
    "dwell-game/src",
    "dwell-look/src",
    "dwell-characters/src",
    "dwell-sim/src",
    "dwell-client/src",
];

/// Where the frame itself lives: these may mirror by hand.
const FRAME_FILES: &[&str] = &["dwell-world/src/face.rs", "dwell-world/src/voxel.rs"];
/// The netcode's face boundary: the only place the wire's face bit is read and written.
const WIRE_FILES: &[&str] = &["dwell-physics/src/net.rs"];
/// This file (its patterns would match themselves).
const SELF: &str = "dwell-world/tests/both_faces.rs";

const BOUNDARY: &str = "// face-boundary:";
const TEST_MARKERS: &[&str] = &["// face-a-only:", "// face-b-only:", "// both-faces:"];

fn crates_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..")
}

fn sources(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            sources(&p, out);
        } else if p.extension().is_some_and(|x| x == "rs" || x == "wgsl") {
            out.push(p);
        }
    }
}

/// Every scanned file: (path relative to `crates/`, contents).
fn files() -> Vec<(String, String)> {
    let root = crates_dir();
    let mut paths = Vec::new();
    for d in SCANNED {
        sources(&root.join(d), &mut paths);
    }
    paths.sort();
    paths
        .into_iter()
        .map(|p| {
            let rel = p
                .strip_prefix(&root)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            let text = fs::read_to_string(&p).unwrap();
            (rel, text)
        })
        .filter(|(rel, _)| rel != SELF)
        .collect()
}

/// A marker with a reason after it.
fn marked(line: &str, marker: &str) -> bool {
    line.find(marker)
        .is_some_and(|i| !line[i + marker.len()..].trim().is_empty())
}

/// Whether `code` holds `name` as a whole identifier.
fn word(code: &str, name: &str) -> bool {
    let ident = |c: char| c.is_ascii_alphanumeric() || c == '_';
    code.match_indices(name).any(|(i, _)| {
        let before = code[..i].chars().next_back().is_none_or(|c| !ident(c));
        let after = code[i + name.len()..]
            .chars()
            .next()
            .is_none_or(|c| !ident(c));
        before && after
    })
}

/// What a line of code mirrors by hand, if anything (comments and imports aside).
fn raw_mirroring(code: &str, wire_ok: bool) -> Option<&'static str> {
    let code = code.split("//").next().unwrap_or("");
    if code.trim_start().starts_with("use ") {
        return None;
    }
    let has = |s: &str| code.contains(s);
    if has("MIRROR_SUM") || has("MIRROR_CHUNK_SUM") || has("FaceFrame::MIRROR") {
        return Some("the mirror's sum");
    }
    if has("2 * MIDPLANE") || has("2.0 * MIDPLANE") || has("* MIDPLANE_Y") {
        return Some("2 × MIDPLANE_Y");
    }
    if let Some(i) = code.find("MIDPLANE_Y") {
        let before = code[..i].trim_end();
        let before = before
            .trim_end_matches("f64::from(")
            .trim_end_matches("dwell_world::")
            .trim_end_matches("crate::")
            .trim_end_matches("world::")
            .trim_end();
        let after = code[i + "MIDPLANE_Y".len()..].trim_start();
        let compares = |s: &str| {
            s.ends_with(">=") || s.ends_with("<=") || s.ends_with('<') || s.ends_with('>')
        };
        if compares(before) || [">=", "<=", "<", ">"].iter().any(|o| after.starts_with(o)) {
            return Some("a face found by comparing with MIDPLANE_Y");
        }
    }
    if has("select(-1.0, 1.0") || has("select(1.0, -1.0") {
        return Some("a WGSL face sign by hand");
    }
    if !wire_ok && word(code, "FACE_B") && !has("face_of_flags(") && !has("face_bit(") {
        return Some("the wire's face bit outside face_of_flags / face_bit");
    }
    None
}

/// Rule 1 over one file's lines, test code included (fixtures go through the frame too).
fn rule_1(rel: &str, text: &str, out: &mut Vec<String>) {
    if FRAME_FILES.contains(&rel) {
        return;
    }
    let wire_ok = WIRE_FILES.contains(&rel);
    let lines: Vec<&str> = text.lines().collect();
    for (i, line) in lines.iter().enumerate() {
        let Some(what) = raw_mirroring(line, wire_ok) else {
            continue;
        };
        let above = if i > 0 { lines[i - 1] } else { "" };
        let below = lines.get(i + 1).copied().unwrap_or("");
        if [*line, above, below].iter().any(|l| marked(l, BOUNDARY)) {
            continue;
        }
        out.push(format!("{rel}:{}: {what}: `{}`", i + 1, line.trim()));
    }
}

const FACE_TOKENS: &[&str] = &[
    "FaceFrame::B",
    "Face::B",
    "FACE_B",
    "MIDPLANE",
    "MIRROR",
    "mirrored(",
    "mirror_y",
    "mirror_chunk",
    "mirror_row",
    "mirror_section",
    "spawn_point_b",
    "face_b",
    ".is_b()",
];

/// The `#[test]` functions in a file: (line of `#[test]`, the lines above it back to the previous
/// item, the body).
fn tests(text: &str) -> Vec<(usize, String, String)> {
    let lines: Vec<&str> = text.lines().collect();
    let mut out = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        if line.trim() != "#[test]" {
            continue;
        }
        // The comments and attributes just above.
        let mut above = String::new();
        let mut k = i;
        while k > 0 {
            let l = lines[k - 1].trim();
            if l.starts_with("//") || l.starts_with("#[") {
                above.push_str(l);
                above.push('\n');
                k -= 1;
            } else {
                break;
            }
        }
        // The body: from the fn line to its closing brace.
        let mut depth = 0i32;
        let mut started = false;
        let mut body = String::new();
        for l in &lines[i + 1..] {
            body.push_str(l);
            body.push('\n');
            for c in l.split("//").next().unwrap_or("").chars() {
                match c {
                    '{' => {
                        depth += 1;
                        started = true;
                    }
                    '}' => depth -= 1,
                    _ => {}
                }
            }
            if started && depth <= 0 {
                break;
            }
        }
        out.push((i + 1, above, body));
    }
    out
}

/// Rule 2 over one file's plain tests.
fn rule_2(rel: &str, text: &str, out: &mut Vec<String>) {
    if rel.starts_with("dwell-physics/tests/") || FRAME_FILES.contains(&rel) {
        return;
    }
    for (line, above, body) in tests(text) {
        let name = body.lines().next().unwrap_or("").trim();
        if KNOWN
            .iter()
            .any(|(f, t, _)| *f == rel && name.contains(&format!("fn {t}(")))
        {
            continue;
        }
        let code: String = body
            .lines()
            .map(|l| l.split("//").next().unwrap_or(""))
            .collect::<Vec<_>>()
            .join("\n");
        let Some(token) = FACE_TOKENS.iter().find(|t| code.contains(**t)) else {
            continue;
        };
        if code.contains("under_test()") {
            continue;
        }
        let said = |s: &str| s.lines().any(|l| TEST_MARKERS.iter().any(|m| marked(l, m)));
        if said(&above) || said(&body) {
            continue;
        }
        out.push(format!(
            "{rel}:{line}: `{name}` uses `{token}` but runs on one face: make it a both_faces! \
             test, loop over FaceFrame::under_test(), or mark it `// face-a-only: <reason>` \
             (or face-b-only / both-faces)"
        ));
    }
}

#[test]
fn face_dependent_code_goes_through_the_face_frame_and_its_tests_run_on_both_faces() {
    let files = files();
    assert!(
        files.len() > 100,
        "the scan found the sources ({} files)",
        files.len()
    );
    let mut problems = Vec::new();
    for (rel, text) in &files {
        rule_1(rel, text, &mut problems);
        rule_2(rel, text, &mut problems);
    }
    assert!(
        problems.is_empty(),
        "{} face-parity problem(s) (BIFACIAL_WORLD.md §11.2):\n{}",
        problems.len(),
        problems.join("\n")
    );
}

#[test]
fn the_check_catches_what_it_is_for() {
    // Rule 1: hand mirroring, unless marked as a boundary.
    let mut out = Vec::new();
    rule_1(
        "dwell-x/src/a.rs",
        "let y = 2.0 * MIDPLANE_Y - eye;\nlet b = y < f64::from(MIDPLANE_Y);\nlet ok = f.y(eye);\n\
         // face-boundary: the LOD's rows\nlet r = MIRROR_SUM - y;\nlet ok = f.y(eye);\n\
         let s = select(-1.0, 1.0, on_a);\nlet bit = flags & player_flags::FACE_B != 0;",
        &mut out,
    );
    assert_eq!(out.len(), 4, "{out:#?}");
    // Rule 2: a face-dependent plain test, unless both-faced or marked.
    let mut out = Vec::new();
    let src = "#[test]\nfn plain() {\n    let y = FaceFrame::B.y(3.0);\n}\n\
               // face-a-only: face A's spawn\n#[test]\nfn marked() {\n    let m = MIDPLANE_Y;\n}\n\
               #[test]\nfn looped() {\n    for f in FaceFrame::under_test() { f.y(FaceFrame::B.y(1.0)); }\n}\n\
               #[test]\nfn unrelated() {\n    assert_eq!(1 + 1, 2);\n}\n";
    rule_2("dwell-x/src/a.rs", src, &mut out);
    assert_eq!(out.len(), 1, "{out:#?}");
    assert!(out[0].contains("plain"), "{out:#?}");
}

/// Plain tests that name a face on purpose, as of the check's arrival (2026-10-20), each with why
/// it is one face's or compares the faces itself: (file, test, reason). New tests use the
/// `both_faces!` macro or a marker instead; this list only shrinks.
const KNOWN: &[(&str, &str, &str)] = &[
    (
        "dwell-client/src/jobs/mod.rs",
        "lod_sections_carry_face_as_surface_where_face_a_has_rows",
        "the job codec's per-face rows",
    ),
    (
        "dwell-client/src/render/camera.rs",
        "the_half_forward_somersault_across_the_midplane",
        "crosses the midplane",
    ),
    (
        "dwell-client/src/render/characters.rs",
        "the_arms_sit_ahead_of_the_eye_whatever_the_view",
        "views on both faces",
    ),
    (
        "dwell-game/src/chunk_stream.rs",
        "meshes_face_bs_chunks_as_mirror_images_face_as_as_they_are",
        "compares the faces",
    ),
    (
        "dwell-game/src/eye.rs",
        "on_face_b_the_eye_is_the_mirror_image",
        "compares the faces",
    ),
    (
        "dwell-game/src/eye.rs",
        "starts_afresh_across_the_midplane",
        "crosses the midplane",
    ),
    (
        "dwell-game/src/input.rs",
        "looks_and_clicks_only_while_the_pointer_is_locked",
        "the turn's handedness on both faces",
    ),
    (
        "dwell-lod/src/grid.rs",
        "the_root_is_centred_on_the_origin_rows_span_the_build_sphere",
        "the grid spans both faces",
    ),
    (
        "dwell-lod/src/grid.rs",
        "knows_the_rim_cells_and_bounds",
        "the grid spans both faces",
    ),
    (
        "dwell-lod/src/grid.rs",
        "face_b_sections_and_their_mirror_origin",
        "the mirror boundary itself",
    ),
    (
        "dwell-lod/src/system/tests.rs",
        "does_not_generate_or_draw_the_far_faces_sections_camera_on_face_b",
        "the other face's culling",
    ),
    (
        "dwell-lod/src/system/tests.rs",
        "meshes_level_7_and_up_sections_for_the_viewers_face_the_other_faces_rows_are_stone",
        "straddling sections, by design (§10)",
    ),
    (
        "dwell-lod/src/system/tests.rs",
        "a_thin_crust_meshes_the_viewers_face_whole_and_none_of_the_other_at_every_level",
        "compares the faces",
    ),
    (
        "dwell-look/src/clouds.rs",
        "offsets_stay_in_one_period_far_from_the_origin_and_drift_with_the_wind",
        "face B's clouds are another part of the noise",
    ),
    (
        "dwell-look/src/fog.rs",
        "stays_finite_at_the_extremes_and_is_off_at_zero_density",
        "face-local heights, the midplane the lowest",
    ),
    (
        "dwell-look/src/lib.rs",
        "the_fog_is_the_horizons_colour_at_every_time_of_day",
        "loops over both faces",
    ),
    (
        "dwell-look/src/lib.rs",
        "face_b_has_night_while_face_a_has_day",
        "compares the faces",
    ),
    (
        "dwell-look/src/lib.rs",
        "light_weights_follow_the_fragments_own_horizon",
        "compares the faces",
    ),
    (
        "dwell-mesh/src/lod_mesher_tests.rs",
        "reverses_the_rows_of_a_section_apron_included_and_back",
        "the mirror boundary itself",
    ),
    (
        "dwell-mesh/src/lod_mesher_tests.rs",
        "meshes_a_ceiling_as_the_turned_over_image_of_a_floor_with_the_top_shading_on_its_underside",
        "the mirror boundary itself",
    ),
    (
        "dwell-mesh/src/lod_mesher_tests.rs",
        "winds_the_turned_over_faces_to_face_the_way_their_normals_point_and_swaps_the_y_skirts",
        "the mirror boundary itself",
    ),
    (
        "dwell-mesh/src/lod_mesher_tests.rs",
        "a_one_cell_step_is_one_even_slope_and_face_b_hangs",
        "compares the faces",
    ),
    (
        "dwell-mesh/src/mesher_tests.rs",
        "puts_the_top_texture_and_shading_on_the_face_toward_the_sky_which_on_face_b_is_minus_y",
        "compares the faces",
    ),
    (
        "dwell-mesh/src/mesher_tests.rs",
        "face_b_shapes_hang_toward_its_sky",
        "face B's spot check beside the parity suite",
    ),
    (
        "dwell-mesh/src/mesher_tests.rs",
        "tall_grass_on_face_b_sits_on_its_support_too",
        "face B's spot check beside the parity suite",
    ),
    (
        "dwell-physics/src/net.rs",
        "controller_state_round_trips",
        "the wire's face bit",
    ),
    (
        "dwell-world/src/fluid.rs",
        "face_b_is_the_mirror_image_its_surface_above_the_cells_bottom",
        "compares the faces",
    ),
    (
        "dwell-world/tests/world.rs",
        "flat_world_layers_and_revisions",
        "the flat world is face A's",
    ),
    (
        "dwell-world/tests/world.rs",
        "breaking_the_block_under_tall_grass_takes_the_grass_with_it",
        "the flat world is face A's",
    ),
    (
        "dwell-world/tests/world.rs",
        "tall_grass_on_face_b_aims_toward_its_ground",
        "face B's spot check beside the parity suite",
    ),
    (
        "dwell-worldgen/tests/lod.rs",
        "rows_cover_the_build_sphere_one_row_at_the_root",
        "the grid spans both faces",
    ),
    (
        "dwell-worldgen/tests/lod.rs",
        "a_face_b_section_agrees_with_the_downsample_of_its_chunks",
        "face B's generator against its chunks",
    ),
    (
        "dwell-worldgen/tests/lod.rs",
        "a_straddling_section_keeps_each_cell_on_its_face",
        "straddles the midplane",
    ),
    (
        "dwell-worldgen/tests/terrain.rs",
        "the_core_is_stone_nothing_below_the_face_or_over_the_build_volume",
        "the build volume spans both faces",
    ),
    (
        "dwell-worldgen/tests/terrain.rs",
        "beyond_the_rim_nothing_is_generated",
        "the build volume spans both faces",
    ),
    (
        "dwell-worldgen/tests/terrain.rs",
        "the_sandbox_is_the_flat_world_in_its_indestructible_floor_block",
        "the sandbox is face A's",
    ),
    (
        "dwell-worldgen/tests/terrain.rs",
        "the_fill_grass_over_dirt_sand_at_the_shore_sea_to_its_level",
        "face A's generator; face B's is its mirror (tests/lod.rs)",
    ),
    (
        "dwell-worldgen/tests/terrain.rs",
        "the_mirror_maps_voxels_and_chunks_and_is_its_own_inverse",
        "the mirror boundary itself",
    ),
    (
        "dwell-worldgen/tests/terrain.rs",
        "a_chunk_of_face_b_is_the_flip_of_its_face_local_chunk",
        "the mirror boundary itself",
    ),
    (
        "dwell-worldgen/tests/terrain.rs",
        "face_b_is_its_own_landscape",
        "compares the faces",
    ),
    (
        "dwell-worldgen/tests/terrain.rs",
        "the_air_test_knows_face_bs_sky_and_rows_outside_the_world",
        "face B's sky",
    ),
    (
        "dwell-worldgen/tests/terrain.rs",
        "the_hills_are_steep_enough_to_slide_and_reach_about_500_m",
        "samples both faces",
    ),
];
