use super::*;

fn k(t_ms: i64, x: f32, y: f32) -> Keyframe {
    Keyframe { t_ms, x, y }
}

#[test]
fn a_path_is_sorted_clamped_and_has_one_keyframe_per_instant() {
    let got = sorted_path(&[
        k(2000, 0.5, 0.5),
        k(0, -1.0, 2.0),
        k(1000, 0.2, 0.3),
        k(1000, 0.7, 0.8), // the same instant given twice: the later one wins
        k(500, f32::NAN, 0.5),
        k(600, 0.5, f32::INFINITY),
    ]);
    assert_eq!(
        got,
        vec![k(0, 0.0, 1.0), k(1000, 0.7, 0.8), k(2000, 0.5, 0.5)]
    );
    assert!(sorted_path(&[]).is_empty());
}

#[test]
fn it_moves_in_straight_lines_and_holds_at_the_ends() {
    let p = [k(1000, 0.2, 0.4), k(3000, 0.6, 0.0), k(4000, 0.6, 1.0)];
    let at = |t| position_at(&p, t, (0.5, 0.5));
    assert_eq!(at(0), (0.2, 0.4), "held before the first keyframe");
    assert_eq!(at(1000), (0.2, 0.4));
    let (x, y) = at(2000); // halfway along the first line
    assert!((x - 0.4).abs() < 1e-6 && (y - 0.2).abs() < 1e-6, "{x},{y}");
    assert_eq!(at(3000), (0.6, 0.0));
    let (x, y) = at(3500);
    assert!((x - 0.6).abs() < 1e-6 && (y - 0.5).abs() < 1e-6, "{x},{y}");
    assert_eq!(at(4000), (0.6, 1.0));
    assert_eq!(at(99_999), (0.6, 1.0), "held after the last");
}

#[test]
fn with_no_path_the_fallback_stands() {
    assert_eq!(position_at(&[], 1234, (0.3, 0.7)), (0.3, 0.7));
    // one keyframe is a place the thing stays
    assert_eq!(
        position_at(&[k(500, 0.1, 0.2)], 9000, (0.9, 0.9)),
        (0.1, 0.2)
    );
}

#[test]
fn a_path_is_checked_before_an_export_starts() {
    assert_eq!(validate_path(&[], 5000), None);
    assert_eq!(
        validate_path(&[k(0, 0.0, 1.0), k(5000, 1.0, 0.0)], 5000),
        None
    );
    for (bad, why) in [
        (k(-1, 0.5, 0.5), "within the clip"),
        (k(5001, 0.5, 0.5), "within the clip"),
        (k(0, 1.5, 0.5), "inside the picture"),
        (k(0, 0.5, -0.1), "inside the picture"),
        (k(0, f32::NAN, 0.5), "not usable"),
    ] {
        let got = validate_path(&[bad], 5000).unwrap();
        assert!(got.contains(why), "{got}");
    }
    let many: Vec<_> = (0..=MAX_PATH_POINTS as i64)
        .map(|i| k(i, 0.5, 0.5))
        .collect();
    assert!(validate_path(&many, 5000).unwrap().contains("at most"));
    assert_eq!(validate_path(&many[..MAX_PATH_POINTS], 5000), None);
}

#[test]
fn the_expression_follows_the_same_line_ffmpeg_will_evaluate() {
    let p = [k(1000, 0.2, 0.4), k(3000, 0.6, 0.0)];
    assert_eq!(
        path_expr(&p, Axis::X),
        "0.20000+(0.40000)*(clip(t,1.000,3.000)-1.000)/2.000"
    );
    assert_eq!(
        path_expr(&p, Axis::Y),
        "0.40000+(-0.40000)*(clip(t,1.000,3.000)-1.000)/2.000"
    );
    // a single keyframe is a constant
    assert_eq!(path_expr(&[k(500, 0.25, 0.75)], Axis::X), "0.25000");
    assert_eq!(path_expr(&[k(500, 0.25, 0.75)], Axis::Y), "0.75000");
    // two lines: one test of the time between them
    let q = [k(0, 0.0, 0.0), k(1000, 1.0, 1.0), k(2000, 0.0, 0.0)];
    let e = path_expr(&q, Axis::X);
    assert!(e.starts_with("if(lt(t,1.000),"), "{e}");
    assert_eq!(e.matches("if(").count(), 1, "{e}");
}

/// Evaluate an expression of the shape `path_expr` builds, so the test does not depend
/// on ffmpeg: it understands only `if`, `lt`, `clip`, `t`, numbers and + - * /.
#[test]
fn the_expression_agrees_with_position_at_all_along_the_clip() {
    let p = [
        k(0, 0.1, 0.9),
        k(700, 0.5, 0.2),
        k(1800, 0.9, 0.2),
        k(2600, 0.3, 0.6),
    ];
    for axis in [Axis::X, Axis::Y] {
        let expr = path_expr(&p, axis);
        for t_ms in (0..=3000).step_by(50) {
            let want = position_at(&p, t_ms, (0.0, 0.0));
            let want = if axis == Axis::X { want.0 } else { want.1 } as f64;
            let got = eval(&expr, t_ms as f64 / 1000.0);
            assert!(
                (got - want).abs() < 1e-4,
                "{axis:?} at {t_ms}: {got} vs {want}\n{expr}"
            );
        }
    }
}

// a tiny evaluator for the expressions above (recursive descent)
fn eval(src: &str, t: f64) -> f64 {
    struct P<'a> {
        s: &'a [u8],
        i: usize,
        t: f64,
    }
    impl P<'_> {
        fn ws(&mut self) {
            while self.i < self.s.len() && self.s[self.i] == b' ' {
                self.i += 1;
            }
        }
        fn eat(&mut self, c: u8) {
            self.ws();
            assert_eq!(
                self.s[self.i],
                c,
                "at {}: {}",
                self.i,
                String::from_utf8_lossy(self.s)
            );
            self.i += 1;
        }
        fn expr(&mut self) -> f64 {
            let mut v = self.term();
            loop {
                self.ws();
                match self.s.get(self.i) {
                    Some(b'+') => {
                        self.i += 1;
                        v += self.term();
                    }
                    Some(b'-') => {
                        self.i += 1;
                        v -= self.term();
                    }
                    _ => return v,
                }
            }
        }
        fn term(&mut self) -> f64 {
            let mut v = self.atom();
            loop {
                self.ws();
                match self.s.get(self.i) {
                    Some(b'*') => {
                        self.i += 1;
                        v *= self.atom();
                    }
                    Some(b'/') => {
                        self.i += 1;
                        v /= self.atom();
                    }
                    _ => return v,
                }
            }
        }
        fn atom(&mut self) -> f64 {
            self.ws();
            let c = self.s[self.i];
            if c == b'(' {
                self.i += 1;
                let v = self.expr();
                self.eat(b')');
                return v;
            }
            if c == b'-' {
                self.i += 1;
                return -self.atom();
            }
            if c == b't' && self.s.get(self.i + 1) != Some(&b'(') {
                self.i += 1;
                return self.t;
            }
            if c.is_ascii_digit() {
                let st = self.i;
                while self.i < self.s.len()
                    && (self.s[self.i].is_ascii_digit() || self.s[self.i] == b'.')
                {
                    self.i += 1;
                }
                return std::str::from_utf8(&self.s[st..self.i])
                    .unwrap()
                    .parse()
                    .unwrap();
            }
            let st = self.i;
            while self.s[self.i].is_ascii_alphabetic() {
                self.i += 1;
            }
            let name = std::str::from_utf8(&self.s[st..self.i])
                .unwrap()
                .to_string();
            self.eat(b'(');
            let mut args = Vec::new();
            // `if` needs its branches evaluated lazily only for cost; evaluating both is fine here
            loop {
                args.push(self.expr());
                self.ws();
                if self.s[self.i] == b',' {
                    self.i += 1;
                } else {
                    break;
                }
            }
            self.eat(b')');
            match name.as_str() {
                "if" => {
                    if args[0] != 0.0 {
                        args[1]
                    } else {
                        args[2]
                    }
                }
                "lt" => f64::from(args[0] < args[1]),
                "clip" => args[0].clamp(args[1], args[2]),
                other => panic!("unknown function {other}"),
            }
        }
    }
    let mut p = P {
        s: src.as_bytes(),
        i: 0,
        t,
    };
    p.expr()
}

/// The deepest nesting of parentheses in an expression.
fn depth(e: &str) -> usize {
    let (mut d, mut max) = (0usize, 0usize);
    for c in e.chars() {
        match c {
            '(' => {
                d += 1;
                max = max.max(d);
            }
            ')' => d -= 1,
            _ => {}
        }
    }
    max
}

#[test]
fn a_long_path_is_a_shallow_tree_not_a_deep_chain() {
    // ffmpeg's parser gives up at about a hundred levels of nesting
    let p: Vec<_> = (0..MAX_PATH_POINTS as i64)
        .map(|i| k(i * 40, (i % 7) as f32 / 7.0, 0.5))
        .collect();
    let e = path_expr(&p, Axis::X);
    assert!(depth(&e) < 30, "nesting {}", depth(&e));
    // and it is still the same function
    for t_ms in (0..4000).step_by(13) {
        let want = position_at(&p, t_ms, (0.0, 0.0)).0 as f64;
        let got = eval(&e, t_ms as f64 / 1000.0);
        assert!((got - want).abs() < 1e-4, "at {t_ms}: {got} vs {want}");
    }
}
