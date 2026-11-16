use super::*;

const W: usize = 160;
const H: usize = 100;
const PATCH: usize = 24;

/// A hash of a lattice point: unrelated to its neighbours.
fn lattice(seed: u32, x: usize, y: usize) -> f32 {
    let mut v = seed
        .wrapping_add((x as u32).wrapping_mul(374_761_393))
        .wrapping_add((y as u32).wrapping_mul(668_265_263));
    v = (v ^ (v >> 13)).wrapping_mul(1_274_126_177);
    ((v ^ (v >> 16)) & 0xFF) as f32
}

/// Deterministic texture with detail on the scale of a few pixels, as a downscaled
/// picture has: random values on a grid, smoothly blended between its points.
fn tex(seed: u32, x: usize, y: usize) -> u8 {
    const CELL: usize = 4;
    let (cx, cy) = (x / CELL, y / CELL);
    let (fx, fy) = (
        (x % CELL) as f32 / CELL as f32,
        (y % CELL) as f32 / CELL as f32,
    );
    let (a, b) = (lattice(seed, cx, cy), lattice(seed, cx + 1, cy));
    let (c, d) = (lattice(seed, cx, cy + 1), lattice(seed, cx + 1, cy + 1));
    let top = a + (b - a) * fx;
    let bottom = c + (d - c) * fx;
    (top + (bottom - top) * fy) as u8
}

/// One frame: a fixed background texture, and a patch of another texture whose top-left
/// is at `at` (none: the patch is not there). `gain` and `lift` change the brightness.
fn frame(at: Option<(i32, i32)>, gain: f32, lift: f32) -> Vec<u8> {
    let mut px = vec![0u8; W * H];
    for y in 0..H {
        for x in 0..W {
            let mut v = tex(1, x, y);
            if let Some((px0, py0)) = at {
                let (dx, dy) = (x as i32 - px0, y as i32 - py0);
                if (0..PATCH as i32).contains(&dx) && (0..PATCH as i32).contains(&dy) {
                    v = tex(2, dx as usize, dy as usize);
                }
            }
            px[y * W + x] = (v as f32 * gain + lift).clamp(0.0, 255.0) as u8;
        }
    }
    px
}

/// Frames of the patch moving along `path`, which gives its top-left for frame `i`.
fn clip(n: usize, path: impl Fn(usize) -> Option<(i32, i32)>) -> Vec<u8> {
    (0..n).flat_map(|i| frame(path(i), 1.0, 0.0)).collect()
}

fn params() -> TrackParams {
    TrackParams {
        template: PATCH,
        radius: 14,
    }
}

/// The centre of the patch whose top-left is `p`.
fn centre(p: (i32, i32)) -> (f32, f32) {
    (
        p.0 as f32 + PATCH as f32 / 2.0,
        p.1 as f32 + PATCH as f32 / 2.0,
    )
}

fn worst_error(found: &Followed, truth: impl Fn(usize) -> (i32, i32)) -> f32 {
    found
        .points
        .iter()
        .map(|p| {
            let (cx, cy) = centre(truth(p.frame));
            (p.x - cx).hypot(p.y - cy)
        })
        .fold(0.0, f32::max)
}

#[test]
fn it_follows_a_steady_movement_to_within_a_pixel() {
    let truth = |i: usize| (10 + 3 * i as i32, 30 + i as i32);
    let frames = clip(40, |i| Some(truth(i)));
    let f = follow(&frames, (W, H), 0, centre(truth(0)), params()).unwrap();
    assert!(!f.lost);
    assert_eq!(f.points.len(), 40);
    assert_eq!(f.points[0].frame, 0);
    let worst = worst_error(&f, truth);
    assert!(worst < 1.0, "worst error {worst} px");
}

#[test]
fn it_follows_a_curve_with_changing_speed() {
    let truth = |i: usize| {
        let t = i as f32;
        (
            20 + (t * 1.6 + 0.02 * t * t) as i32,
            40 + (18.0 * (t / 6.0).sin()) as i32,
        )
    };
    let frames = clip(36, |i| Some(truth(i)));
    let f = follow(&frames, (W, H), 0, centre(truth(0)), params()).unwrap();
    assert!(!f.lost, "lost at {:?}", f.points.last());
    assert_eq!(f.points.len(), 36);
    assert!(worst_error(&f, truth) < 1.5);
}

#[test]
fn a_change_of_brightness_does_not_throw_it() {
    let truth = |i: usize| (20 + 2 * i as i32, 40);
    // each frame a little brighter and with a different contrast
    let frames: Vec<u8> = (0..30)
        .flat_map(|i| frame(Some(truth(i)), 1.0 + i as f32 * 0.02, i as f32 * 1.5))
        .collect();
    let f = follow(&frames, (W, H), 0, centre(truth(0)), params()).unwrap();
    assert!(!f.lost);
    assert_eq!(f.points.len(), 30);
    assert!(worst_error(&f, truth) < 1.5);
}

#[test]
fn it_starts_from_the_chosen_frame_and_goes_forward() {
    let truth = |i: usize| (10 + 2 * i as i32, 20 + i as i32);
    let frames = clip(30, |i| Some(truth(i)));
    let f = follow(&frames, (W, H), 12, centre(truth(12)), params()).unwrap();
    assert_eq!(f.points[0].frame, 12);
    assert_eq!(f.points.len(), 30 - 12);
    assert!(worst_error(&f, truth) < 1.0);
}

#[test]
fn it_says_when_it_has_lost_the_spot_and_stops() {
    let truth = |i: usize| (10 + 3 * i as i32, 30);
    // the patch is gone from frame 15 on
    let frames = clip(40, |i| (i < 15).then(|| truth(i)));
    let f = follow(&frames, (W, H), 0, centre(truth(0)), params()).unwrap();
    assert!(f.lost);
    // it held still for the missed frames before giving up, then stopped
    assert!(
        f.points.len() >= 15 && f.points.len() <= 15 + LOST_FRAMES,
        "{}",
        f.points.len()
    );
    let last = f.points.last().unwrap();
    assert!(last.score < LOST_BELOW);
}

#[test]
fn a_flat_spot_is_refused_with_a_reason() {
    let frames = vec![100u8; W * H * 5];
    let err = follow(&frames, (W, H), 0, (50.0, 50.0), params()).unwrap_err();
    assert!(err.to_string().contains("nothing to follow"), "{err}");
}

#[test]
fn a_start_after_the_last_frame_is_an_error() {
    let frames = vec![0u8; W * H * 3];
    assert!(follow(&frames, (W, H), 3, (10.0, 10.0), params()).is_err());
    assert!(follow(&[], (W, H), 0, (10.0, 10.0), params()).is_err());
}

#[test]
fn a_spot_at_the_edge_of_the_picture_is_tracked_without_going_outside_it() {
    let truth = |i: usize| (i as i32, i as i32);
    let frames = clip(20, |i| Some(truth(i)));
    // the click is right in the corner
    let f = follow(&frames, (W, H), 0, (1.0, 1.0), params()).unwrap();
    assert!(!f.lost);
    for p in &f.points {
        assert!(
            p.x >= 0.0 && p.x <= W as f32 && p.y >= 0.0 && p.y <= H as f32,
            "{p:?}"
        );
    }
    assert!(worst_error(&f, truth) < 1.5);
}

#[test]
fn it_never_reads_outside_the_frame_when_the_thing_leaves() {
    // the patch runs off the right-hand edge; the tracker must stop or hold, not panic
    let truth = |i: usize| (100 + 8 * i as i32, 40);
    let frames = clip(20, |i| Some(truth(i)));
    let f = follow(&frames, (W, H), 0, centre(truth(0)), params()).unwrap();
    assert!(f.points.iter().all(|p| p.x <= W as f32));
}

fn k(t_ms: i64, x: f32, y: f32) -> Keyframe {
    Keyframe { t_ms, x, y }
}

#[test]
fn a_straight_movement_is_just_its_two_ends() {
    let line: Vec<_> = (0..50).map(|i| k(i * 100, i as f32 / 100.0, 0.5)).collect();
    assert_eq!(
        simplify(&line, 0.004, MAX_PATH_POINTS),
        vec![line[0], line[49]]
    );
}

#[test]
fn a_corner_is_kept() {
    // right until the 19th point, then down from it: a corner there and nowhere else
    let mut p: Vec<_> = (0..20).map(|i| k(i * 100, i as f32 / 40.0, 0.2)).collect();
    p.extend((20..40).map(|i| k(i * 100, 19.0 / 40.0, 0.2 + (i - 19) as f32 / 40.0)));
    let s = simplify(&p, 0.004, MAX_PATH_POINTS);
    assert_eq!(s.len(), 3, "{s:?}");
    assert_eq!((s[1].t_ms, s[1].x), (1900, 19.0 / 40.0));
}

#[test]
fn the_thinned_path_stays_within_the_tolerance_of_the_original() {
    let wiggle: Vec<_> = (0..120)
        .map(|i| {
            let t = i as f32 / 20.0;
            k(
                i * 50,
                0.5 + 0.3 * t.sin() * 0.5,
                0.5 + 0.2 * (t * 1.7).cos(),
            )
        })
        .collect();
    let tol = 0.01;
    let s = simplify(&wiggle, tol, MAX_PATH_POINTS);
    assert!(s.len() < wiggle.len() / 2, "{} keyframes", s.len());
    for p in &wiggle {
        let (x, y) = position_at(&s, p.t_ms, (0.0, 0.0));
        assert!((p.x - x).hypot(p.y - y) <= tol + 1e-4, "at {} ms", p.t_ms);
    }
}

#[test]
fn a_path_is_never_longer_than_a_path_may_be() {
    // pure noise cannot be described by a few keyframes at a small tolerance
    let noise: Vec<_> = (0..600)
        .map(|i| {
            k(
                i * 20,
                tex(3, i as usize, 0) as f32 / 255.0,
                tex(4, i as usize, 0) as f32 / 255.0,
            )
        })
        .collect();
    let s = simplify(&noise, 0.001, MAX_PATH_POINTS);
    assert!(s.len() <= MAX_PATH_POINTS, "{}", s.len());
    assert_eq!(
        (s[0], *s.last().unwrap()),
        (noise[0], *noise.last().unwrap())
    );
}

#[test]
fn a_path_is_in_the_clips_own_time_and_in_fractions_of_the_picture() {
    let pts: Vec<_> = (0..13)
        .map(|i| Point {
            frame: 6 + i,
            x: 40.0 + 4.0 * i as f32,
            y: 50.0,
            score: 1.0,
        })
        .collect();
    // frame 6 of 12 fps is 500 ms into the clip; the picture is 160 x 100
    let path = to_path(&pts, (W, H), 12, 500, 0.001, MAX_PATH_POINTS);
    assert_eq!(path.first().unwrap().t_ms, 500);
    assert_eq!(path.last().unwrap().t_ms, 500 + 12 * 1000 / 12);
    assert!((path[0].x - 0.25).abs() < 1e-5 && (path[0].y - 0.5).abs() < 1e-5);
    assert!((path.last().unwrap().x - 88.0 / 160.0).abs() < 1e-5);
    assert!(to_path(&[], (W, H), 12, 0, 0.001, MAX_PATH_POINTS).is_empty());
}

#[test]
fn the_decode_size_keeps_the_aspect_and_is_even() {
    assert_eq!(track_dims(1920, 1080), (320, 180));
    assert_eq!(track_dims(1080, 1920), (180, 320));
    assert_eq!(track_dims(640, 480), (320, 240));
    let (w, h) = track_dims(1001, 333);
    assert!(w % 2 == 0 && h % 2 == 0 && w <= TRACK_SIDE && h <= TRACK_SIDE);
}

#[test]
fn a_whole_clip_is_followed_from_a_moment_and_reports_how_far() {
    let truth = |i: usize| (10 + 2 * i as i32, 30);
    let frames = clip(24, |i| Some(truth(i))); // 2 s at 12 fps
    let c = centre(truth(6));
    let req = TrackRequest {
        t0_ms: 500,
        x: c.0 / W as f32,
        y: c.1 / H as f32,
        size: PATCH as f32 / W as f32,
        max_points: MAX_PATH_POINTS,
    };
    let out = track_decoded(&frames, (W, H), &req).unwrap();
    assert!(!out.lost);
    assert_eq!(out.path.first().unwrap().t_ms, 500);
    // from 0.5 s to the last frame (frame 23, 1.917 s)
    assert_eq!(out.tracked_ms, (23 - 6) * 1000 / 12);
    // the keyframes reproduce the movement
    for i in 6..24 {
        let (x, _) = position_at(&out.path, i as i64 * 1000 / 12, (0.0, 0.0));
        let want = centre(truth(i)).0 / W as f32;
        assert!((x - want).abs() < 0.012, "frame {i}: {x} vs {want}");
    }
    assert!(track_decoded(&[], (W, H), &req).is_err());
}

#[test]
fn a_start_moment_past_the_end_follows_from_the_last_frame() {
    let frames = clip(10, |i| Some((10 + i as i32, 30)));
    let req = TrackRequest {
        t0_ms: 99_000,
        x: 0.2,
        y: 0.4,
        size: 0.15,
        max_points: MAX_PATH_POINTS,
    };
    let out = track_decoded(&frames, (W, H), &req).unwrap();
    assert_eq!(out.tracked_ms, 0);
    assert_eq!(out.path.len(), 1);
}

#[test]
fn the_most_keyframes_asked_for_is_respected() {
    // a smooth wiggle needs many keyframes at a small tolerance
    let wiggle: Vec<_> = (0..200)
        .map(|i| {
            let t = i as f32 / 10.0;
            k(i * 50, 0.5 + 0.4 * t.sin(), 0.5 + 0.3 * (t * 1.3).cos())
        })
        .collect();
    assert!(simplify(&wiggle, 0.0005, MAX_PATH_POINTS).len() > 20);
    let few = simplify(&wiggle, 0.0005, 12);
    assert!(few.len() <= 12, "{}", few.len());
    // the ends are always kept, and a limit under two still gives both
    assert_eq!(
        (few[0], *few.last().unwrap()),
        (wiggle[0], *wiggle.last().unwrap())
    );
    assert_eq!(simplify(&wiggle, 0.0005, 0).len(), 2);
    // a limit over the most a path may have is that most
    assert!(simplify(&wiggle, 0.0001, 10_000).len() <= MAX_PATH_POINTS);
}
