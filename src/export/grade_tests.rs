use super::*;

#[test]
fn a_plain_grade_changes_nothing() {
    let g = Grade::default();
    assert!(g.is_plain());
    assert!(g.picture_filters().is_empty() && g.fade_filters(5000).is_empty());
    assert_eq!(
        core_chain_graded(480, 15, &CaptionSpec::none(270), &g, 5000),
        core_chain(480, 15, &CaptionSpec::none(270))
    );
    assert_eq!(
        layers_base_graded(480, 15, &g),
        "fps=15,scale=480:-2:flags=lanczos"
    );
}

#[test]
fn colour_and_mirror_go_right_after_the_sizing_and_fades_go_last() {
    let g = Grade {
        brightness: 0.1,
        contrast: 1.2,
        saturation: 0.5,
        tone: Tone::Bw,
        mirror: true,
        fade_in_ms: 500,
        fade_out_ms: 1000,
    };
    assert!(!g.is_plain());
    let chain = core_chain_graded(480, 15, &CaptionSpec::none(270), &g, 4000);
    assert_eq!(
        chain,
        "fps=15,scale=480:-2:flags=lanczos,eq=brightness=0.100:contrast=1.200:saturation=0.500,hue=s=0,hflip,\
         fade=t=in:st=0:d=0.500,fade=t=out:st=3.000:d=1.000"
    );
    assert_eq!(
        layers_base_graded(480, 15, &g),
        "fps=15,scale=480:-2:flags=lanczos,eq=brightness=0.100:contrast=1.200:saturation=0.500,hue=s=0,hflip"
    );
}

#[test]
fn sepia_is_a_channel_mix_and_only_changed_values_make_an_eq() {
    let g = Grade {
        tone: Tone::Sepia,
        ..Grade::default()
    };
    assert_eq!(g.picture_filters().len(), 1);
    assert!(g.picture_filters()[0].starts_with("colorchannelmixer="));
    let only_contrast = Grade {
        contrast: 1.3,
        ..Grade::default()
    };
    assert_eq!(
        only_contrast.picture_filters(),
        vec!["eq=brightness=0.000:contrast=1.300:saturation=1.000"]
    );
}

#[test]
fn a_grade_is_checked() {
    assert_eq!(validate_grade(&Grade::default(), 5000), None);
    let bad = |g: Grade| validate_grade(&g, 5000).is_some();
    assert!(bad(Grade {
        brightness: 1.5,
        ..Grade::default()
    }));
    assert!(bad(Grade {
        brightness: f32::NAN,
        ..Grade::default()
    }));
    assert!(bad(Grade {
        contrast: -0.1,
        ..Grade::default()
    }));
    assert!(bad(Grade {
        contrast: 2.5,
        ..Grade::default()
    }));
    assert!(bad(Grade {
        saturation: 3.1,
        ..Grade::default()
    }));
    assert!(bad(Grade {
        fade_in_ms: -1,
        ..Grade::default()
    }));
    assert!(bad(Grade {
        fade_in_ms: 3000,
        fade_out_ms: 3000,
        ..Grade::default()
    }));
    assert_eq!(
        validate_grade(
            &Grade {
                fade_in_ms: 2500,
                fade_out_ms: 2500,
                ..Grade::default()
            },
            5000
        ),
        None
    );
}

#[test]
fn it_reads_from_json_with_everything_optional() {
    let g: Grade = serde_json::from_str(r#"{"tone":"sepia","mirror":true}"#).unwrap();
    assert_eq!(
        (g.tone, g.mirror, g.contrast, g.saturation),
        (Tone::Sepia, true, 1.0, 1.0)
    );
    let g: Grade = serde_json::from_str("{}").unwrap();
    assert_eq!(g, Grade::default());
}

#[test]
fn a_gif_with_a_fade_is_made_plain_rgb_before_its_palette() {
    let plain = video_filter(Format::Gif, "fps=15");
    assert!(!plain.contains("format=rgb24"), "{plain}");
    let faded = video_filter(Format::Gif, "fps=15,fade=t=in:st=0:d=0.500");
    assert!(faded.contains(",format=rgb24,split[s0][s1]"), "{faded}");
    let after = finishing_filter(Format::Gif, "fade=t=out:st=1.000:d=1.000");
    assert!(after.contains("format=rgb24,split"), "{after}");
    assert_eq!(
        video_filter(Format::Mp4, "fps=15,fade=t=in:st=0:d=0.500"),
        "fps=15,fade=t=in:st=0:d=0.500"
    );
}
