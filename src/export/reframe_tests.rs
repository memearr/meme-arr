use super::*;

fn crop(x: f32, y: f32, w: f32, h: f32) -> Crop {
    Crop { x, y, w, h }
}

#[test]
fn speed_is_checked() {
    assert!(validate_speed(1.0).is_none());
    assert!(validate_speed(0.25).is_none());
    assert!(validate_speed(4.0).is_none());
    assert!(validate_speed(0.2).is_some());
    assert!(validate_speed(4.5).is_some());
    assert!(validate_speed(f32::NAN).is_some());
    assert!(validate_speed(0.0).is_some());
}

#[test]
fn a_crop_must_sit_inside_the_picture_and_keep_enough_of_it() {
    assert!(validate_crop(None).is_none());
    assert!(validate_crop(Some(&crop(0.0, 0.0, 1.0, 1.0))).is_none());
    assert!(validate_crop(Some(&crop(0.3, 0.0, 0.5625 * 0.5625, 1.0))).is_none());
    assert!(validate_crop(Some(&crop(-0.1, 0.0, 0.5, 0.5))).is_some());
    assert!(validate_crop(Some(&crop(0.6, 0.0, 0.5, 1.0))).is_some());
    assert!(validate_crop(Some(&crop(0.0, 0.0, 0.1, 1.0))).is_some());
    assert!(validate_crop(Some(&crop(0.0, 0.0, f32::NAN, 1.0))).is_some());
}

#[test]
fn the_picture_is_made_wide_enough_that_the_window_comes_out_the_width_asked() {
    assert_eq!(compose_width(480, None), 480);
    assert_eq!(compose_width(481, None), 480);
    // a 9:16 window of a 16:9 picture keeps 31.6% of its width
    let c = crop(0.34, 0.0, 0.3164, 1.0);
    let w = compose_width(480, Some(&c));
    assert_eq!(w % 2, 0);
    assert!((w as f32 * 0.3164 - 480.0).abs() < 2.0, "{w}");
}

#[test]
fn the_finishing_filters_crop_then_change_the_speed() {
    assert!(finishing_filters(480, 15, None, 1.0).is_empty());
    let f = finishing_filters(480, 15, Some(&crop(0.25, 0.1, 0.5, 0.8)), 2.0);
    assert_eq!(
        f,
        vec![
            "crop=trunc(iw*0.5000/2)*2:trunc(ih*0.8000/2)*2:trunc(iw*0.2500):trunc(ih*0.1000)",
            "scale=480:-2:flags=lanczos",
            "setpts=PTS/2.0000",
            "fps=15",
        ]
    );
    // speed alone, and crop alone
    assert_eq!(
        finishing_filters(480, 10, None, 0.5),
        vec!["setpts=PTS/0.5000", "fps=10"]
    );
    assert_eq!(
        finishing_filters(480, 10, Some(&crop(0.0, 0.0, 0.5, 0.5)), 1.0).len(),
        2
    );
}

#[test]
fn the_sound_is_sped_with_atempo_chained_beyond_its_range() {
    assert_eq!(tempo_filter(1.0), "");
    assert_eq!(tempo_filter(1.5), ",atempo=1.5000");
    assert_eq!(tempo_filter(0.75), ",atempo=0.7500");
    assert_eq!(tempo_filter(4.0), ",atempo=2.0,atempo=2.0000");
    assert_eq!(tempo_filter(0.25), ",atempo=0.5,atempo=0.5000");
    assert_eq!(tempo_filter(3.0), ",atempo=2.0,atempo=1.5000");
    assert_eq!(tempo_filter(f32::NAN), "");
}
