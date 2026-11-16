use super::*;

fn shape(shape: &str) -> Layer {
    Layer {
        id: "l".into(),
        kind: LayerKind::Shape {
            shape: shape.into(),
            color: None,
        },
        start_ms: 0,
        end_ms: None,
        x: 0.5,
        y: 0.5,
        size: 0.2,
        rotation: 0.0,
        opacity: 1.0,
    }
}

fn media(asset: &str) -> Layer {
    Layer {
        kind: LayerKind::Media {
            asset: asset.into(),
        },
        ..shape("arrow")
    }
}

fn still(path: &str) -> LayerSource {
    LayerSource {
        path: path.into(),
        motion: false,
    }
}

#[test]
fn a_layer_reads_from_json_with_sensible_defaults() {
    let l: Layer = serde_json::from_str(r#"{"kind":"shape","shape":"arrow"}"#).unwrap();
    assert_eq!(
        l.kind,
        LayerKind::Shape {
            shape: "arrow".into(),
            color: None
        }
    );
    assert_eq!(
        (l.x, l.y, l.size, l.rotation, l.opacity),
        (0.5, 0.5, 0.2, 0.0, 1.0)
    );
    assert_eq!((l.start_ms, l.end_ms), (0, None));
    let m: Layer = serde_json::from_str(
        r#"{"kind":"media","asset":"a-1.png","start_ms":500,"end_ms":900,"x":0.1,"rotation":45}"#,
    )
    .unwrap();
    assert_eq!(
        m.kind,
        LayerKind::Media {
            asset: "a-1.png".into()
        }
    );
    assert_eq!(
        (m.start_ms, m.end_ms, m.x, m.rotation),
        (500, Some(900), 0.1, 45.0)
    );
    // an unknown kind is refused, not guessed at
    assert!(serde_json::from_str::<Layer>(r#"{"kind":"video"}"#).is_err());
}

#[test]
fn layers_are_checked_before_an_export_starts() {
    let ok = |l: Layer| validate_layers(&[l], 5000);
    assert_eq!(ok(shape("arrow")), None);
    assert_eq!(ok(media("a-1.png")), None);
    assert!(ok(shape("triangle")).unwrap().contains("unknown shape"));
    assert!(ok(media(" ")).unwrap().contains("no picture"));
    let mut l = shape("arrow");
    l.kind = LayerKind::Shape {
        shape: "arrow".into(),
        color: Some("red".into()),
    };
    assert!(ok(l).unwrap().contains("#RRGGBB"));
    for bad_size in [0.0, 0.001, 3.0, f32::NAN] {
        let mut l = shape("arrow");
        l.size = bad_size;
        assert!(ok(l).is_some(), "{bad_size}");
    }
    let mut l = shape("arrow");
    l.start_ms = 5000; // not within a 5 s clip
    assert!(ok(l).unwrap().contains("start within"));
    let mut l = shape("arrow");
    l.start_ms = 1000;
    l.end_ms = Some(1000);
    assert!(ok(l).unwrap().contains("end after"));
    let mut l = shape("arrow");
    l.x = f32::INFINITY;
    assert!(ok(l).is_some());
    assert!(validate_layers(&vec![shape("arrow"); MAX_LAYERS + 1], 5000)
        .unwrap()
        .contains("at most"));
    assert_eq!(
        validate_layers(&vec![shape("arrow"); MAX_LAYERS], 5000),
        None
    );
    assert_eq!(validate_layers(&[], 5000), None);
}

#[test]
fn the_graph_sizes_the_clip_then_lays_each_layer_over_it_in_order() {
    let mut a = shape("arrow");
    a.start_ms = 500;
    a.end_ms = Some(2500);
    a.x = 0.25;
    a.y = 0.75;
    a.size = 0.3;
    let b = media("b.png");
    let plan = plan_layers(
        &[a, b],
        &[still("/t/a.png"), still("/t/b.png")],
        480,
        15,
        4000,
        "",
    );
    let g = &plan.graph;
    assert!(
        g.starts_with("[0:v]fps=15,scale=480:-2:flags=lanczos[b0];"),
        "{g}"
    );
    // the first layer: starts at 0.5 s, 144 px wide (0.3 of 480), centred at 25%, 75%
    assert!(
        g.contains("[1:v]setpts=PTS-STARTPTS+0.500/TB,format=rgba,scale=144:-1:flags=lanczos[l1]"),
        "{g}"
    );
    assert!(
        g.contains(
            "[b0][l1]overlay=x='main_w*0.2500-overlay_w/2':y='main_h*0.7500-overlay_h/2':\
             enable='between(t,0.500,2.500)':shortest=1[b1]"
        ),
        "{g}"
    );
    // the second lays over the first's result and, with no end, lasts the whole clip
    assert!(g.contains("[b1][l2]overlay="), "{g}");
    assert!(g.contains("enable='between(t,0.000,4.000)'"), "{g}");
    // nothing to run on the picture afterwards, but it still ends in [out]
    assert!(g.ends_with(";[b2]null[out]"), "{g}");
}

#[test]
fn stills_are_held_and_motion_loops_and_each_input_is_bounded() {
    let plan = plan_layers(
        &[shape("arrow"), media("m.mp4")],
        &[
            still("/t/a.png"),
            LayerSource {
                path: "/t/m.mp4".into(),
                motion: true,
            },
        ],
        480,
        15,
        3000,
        "",
    );
    assert_eq!(
        plan.input_args,
        [
            "-loop",
            "1",
            "-t",
            "3.000",
            "-i",
            "/t/a.png",
            "-stream_loop",
            "-1",
            "-t",
            "3.000",
            "-i",
            "/t/m.mp4"
        ]
        .map(String::from)
    );
}

#[test]
fn a_turn_and_a_fade_add_their_filters_only_when_asked_for() {
    let plain = plan_layers(&[shape("arrow")], &[still("/t/a.png")], 480, 15, 3000, "");
    assert!(!plain.graph.contains("rotate="));
    assert!(!plain.graph.contains("colorchannelmixer"));
    let mut l = shape("arrow");
    l.rotation = 90.0;
    l.opacity = 0.5;
    let g = plan_layers(&[l], &[still("/t/a.png")], 480, 15, 3000, "").graph;
    assert!(
        g.contains("rotate=a=1.57080:c=none:ow='rotw(1.57080)':oh='roth(1.57080)'"),
        "{g}"
    );
    assert!(g.contains("colorchannelmixer=aa=0.500"), "{g}");
    // a full turn is no turn
    let mut full = shape("arrow");
    full.rotation = 360.0;
    assert!(
        !plan_layers(&[full], &[still("/t/a.png")], 480, 15, 3000, "")
            .graph
            .contains("rotate=")
    );
}

#[test]
fn what_runs_afterwards_goes_on_the_finished_picture() {
    let plan = plan_layers(
        &[shape("arrow")],
        &[still("/t/a.png")],
        320,
        10,
        2000,
        "ass='/t/c.ass':fontsdir='/f',split[s0][s1];[s0]palettegen[p];[s1][p]paletteuse",
    );
    assert!(
        plan.graph
            .ends_with(";[b1]ass='/t/c.ass':fontsdir='/f',split[s0][s1];[s0]palettegen[p];[s1][p]paletteuse[out]"),
        "{}",
        plan.graph
    );
}

#[test]
fn times_and_places_are_kept_inside_the_clip_and_the_picture() {
    let mut l = shape("arrow");
    l.start_ms = 1500;
    l.end_ms = Some(99_000); // far past the end
    l.x = 3.0;
    l.y = -1.0;
    let g = plan_layers(&[l], &[still("/t/a.png")], 480, 15, 2000, "").graph;
    assert!(g.contains("enable='between(t,1.500,2.000)'"), "{g}");
    assert!(
        g.contains("main_w*1.0000") && g.contains("main_h*0.0000"),
        "{g}"
    );
}

#[test]
fn the_argv_maps_the_graph_and_the_audio_only_when_it_is_wanted() {
    let plan = LayerPlan {
        input_args: vec!["-loop".into(), "1".into(), "-i".into(), "/t/a.png".into()],
        graph: "G[out]".into(),
    };
    let codec = vec!["-c:v".to_string(), "libx264".into()];
    let silent = export_args_with_layers(
        "/m/f.mkv", 1000, 4000, &plan, false, &codec, None, "/o/x.mp4",
    );
    let at = |a: &[String], s: &str| a.iter().position(|x| x == s).unwrap();
    // the clip is seeked before its own input, then the layer input, then the graph
    assert!(at(&silent, "-ss") < at(&silent, "-i"));
    assert_eq!(silent[at(&silent, "-ss") + 1], "1.000");
    assert_eq!(silent[at(&silent, "-t") + 1], "3.000");
    assert!(at(&silent, "/m/f.mkv") < at(&silent, "/t/a.png"));
    assert_eq!(silent[at(&silent, "-filter_complex") + 1], "G[out]");
    assert_eq!(silent[at(&silent, "-map") + 1], "[out]");
    assert!(!silent.iter().any(|a| a == "0:a:0?"));
    assert_eq!(silent.last().unwrap(), "/o/x.mp4");
    let loud = export_args_with_layers(
        "/m/f.mkv",
        1000,
        4000,
        &plan,
        true,
        &codec,
        Some("T"),
        "/o/x.mp4",
    );
    assert!(loud.iter().any(|a| a == "0:a:0?"));
    // the privacy options still apply
    assert!(loud.iter().any(|a| a == "-map_metadata"));
    assert!(loud.iter().any(|a| a == "title=T"));
}

#[test]
fn shapes_are_written_to_temp_files_and_cleaned_up() {
    let dir = tempfile::tempdir().unwrap();
    let mut green = shape("ring");
    green.kind = LayerKind::Shape {
        shape: "ring".into(),
        color: Some("#00ff00".into()),
    };
    let layers = [shape("arrow"), green, media("m-1.png")];
    let (sources, files) = resolve_layers(&layers, dir.path(), "job1", &|id| {
        (id == "m-1.png").then(|| still("/store/m-1.png"))
    })
    .unwrap();
    assert_eq!(sources.len(), 3);
    assert!(sources[0].path.is_file() && sources[1].path.is_file());
    assert_eq!(sources[2].path, PathBuf::from("/store/m-1.png"));
    let written = files.0.clone();
    assert_eq!(written.len(), 2);
    // the ring is green, the arrow the default orange
    assert_ne!(
        std::fs::read(&sources[0].path).unwrap(),
        std::fs::read(&sources[1].path).unwrap()
    );
    drop(files);
    assert!(written.iter().all(|p| !p.exists()));
}

#[test]
fn a_picture_that_is_gone_is_an_error_naming_the_layer() {
    let dir = tempfile::tempdir().unwrap();
    let err = resolve_layers(&[media("gone.png")], dir.path(), "j", &|_| None)
        .err()
        .unwrap();
    assert!(err.to_string().contains("layer 1") && err.to_string().contains("gone.png"));
}
