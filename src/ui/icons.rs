//! The inline SVG icons of the old pages (src/index.html, toast.html, settings.html on `old-electron`).
use super::svg::{Icon, Shape::*};

const fn stroked(vb: (f32, f32), stroke: f32, round_cap: bool, round_join: bool, shapes: &'static [super::svg::Shape]) -> Icon {
    Icon { vb, stroke, round_cap, round_join, shapes }
}

// ---------- card and detail panel ----------
pub const CLOCK: Icon = stroked((16.0, 16.0), 1.6, true, false, &[Circle(8.0, 8.0, 6.3), P("M8 4.6V8l2.3 1.5")]);
pub const COIN: Icon = stroked(
    (16.0, 16.0),
    1.6,
    false,
    false,
    &[Ellipse(8.0, 5.0, 5.5, 2.3), P("M2.5 5v6c0 1.3 2.5 2.3 5.5 2.3s5.5-1 5.5-2.3V5M2.5 8c0 1.3 2.5 2.3 5.5 2.3s5.5-1 5.5-2.3")],
);
pub const MORE: Icon = stroked((4.0, 16.0), 0.0, false, false, &[Circle(2.0, 3.0, 1.5), Circle(2.0, 8.0, 1.5), Circle(2.0, 13.0, 1.5)]);
pub const WIFI: Icon = stroked(
    (24.0, 24.0),
    2.6,
    true,
    false,
    &[P("M2.5 9a14 14 0 0 1 19 0M6 12.8a9 9 0 0 1 12 0M9.3 16.4a4.4 4.4 0 0 1 5.4 0"), FillCircle(12.0, 19.6, 1.4)],
);
pub const WIFI_OFF: Icon = stroked(
    (24.0, 24.0),
    2.6,
    true,
    false,
    &[P("M2.5 9a14 14 0 0 1 19 0M6 12.8a9 9 0 0 1 12 0M9.3 16.4a4.4 4.4 0 0 1 5.4 0M3 3l18 18"), FillCircle(12.0, 19.6, 1.4)],
);

// ---------- update toast ----------
pub const UP: Icon = stroked((24.0, 24.0), 2.4, true, true, &[P("M12 19V5M5.5 11.5 12 5l6.5 6.5")]);
pub const DOWN: Icon = stroked((24.0, 24.0), 2.4, true, true, &[P("M12 5v14M5.5 12.5 12 19l6.5-6.5")]);
pub const SPIN: Icon = stroked((24.0, 24.0), 2.4, true, false, &[P("M12 3a9 9 0 1 1-9 9")]);
pub const WARN: Icon = stroked((24.0, 24.0), 2.4, true, false, &[P("M12 7v6M12 17h.01")]);

// ---------- app window ----------
pub const CLOSE: Icon = stroked((12.0, 12.0), 1.5, true, false, &[P("M2 2l8 8M10 2l-8 8")]);
pub const EYE: Icon = stroked(
    (24.0, 24.0),
    1.8,
    true,
    true,
    &[P("M2 12s3.6-7 10-7 10 7 10 7-3.6 7-10 7S2 12 2 12z"), Circle(12.0, 12.0, 3.0)],
);
pub const GLOBE: Icon = stroked(
    (24.0, 24.0),
    1.8,
    true,
    true,
    &[Circle(12.0, 12.0, 9.0), P("M3 12h18M12 3a14 14 0 0 1 0 18M12 3a14 14 0 0 0 0 18")],
);
pub const LAYOUT: Icon = stroked((24.0, 24.0), 1.8, false, true, &[Rect(3.0, 8.0, 11.0, 8.0, 2.0), Rect(17.0, 3.0, 4.0, 18.0, 1.5)]);
pub const SIZE: Icon = stroked((24.0, 24.0), 1.8, true, true, &[P("M15 3h6v6M9 21H3v-6M21 3l-7 7M3 21l7-7")]);
pub const OPACITY: Icon = stroked(
    (24.0, 24.0),
    1.8,
    false,
    false,
    &[Circle(12.0, 12.0, 9.0), FillP("M12 3a9 9 0 0 1 0 18z"), P("M12 3a9 9 0 0 1 0 18z")],
);
pub const POWER: Icon = stroked((24.0, 24.0), 1.8, true, false, &[P("M12 3v8M7.5 6.3a7 7 0 1 0 9 0")]);
pub const WIFI_ROW: Icon = stroked(
    (24.0, 24.0),
    2.0,
    true,
    false,
    &[P("M2.5 9a14 14 0 0 1 19 0M6 12.8a9 9 0 0 1 12 0M9.3 16.4a4.4 4.4 0 0 1 5.4 0"), FillCircle(12.0, 19.6, 1.2)],
);
pub const SEG_H: Icon = stroked((14.0, 10.0), 1.5, false, false, &[Rect(0.75, 0.75, 12.5, 8.5, 2.0)]);
pub const SEG_V: Icon = stroked((10.0, 14.0), 1.5, false, false, &[Rect(0.75, 0.75, 8.5, 12.5, 2.0)]);
pub const UPDATE: Icon = stroked((24.0, 24.0), 2.0, true, true, &[P("M12 17V7M7.5 11.5 12 7l4.5 4.5"), Circle(12.0, 12.0, 9.5)]);
pub const REFRESH: Icon = stroked((24.0, 24.0), 2.0, true, true, &[P("M20 11a8 8 0 1 0-2.3 5.7M20 4v7h-7")]);
pub const SNAP: Icon = stroked((24.0, 24.0), 2.0, true, true, &[Rect(3.0, 3.0, 18.0, 18.0, 3.0), FillRect(12.0, 12.0, 6.0, 6.0, 1.0)]);
pub const RESTART: Icon = stroked((24.0, 24.0), 2.0, true, true, &[P("M4 13a8 8 0 1 0 2.3-5.7M4 4v5h5")]);
pub const QUIT: Icon = stroked((24.0, 24.0), 2.0, true, false, &[P("M12 3v8M7.5 6.3a7 7 0 1 0 9 0")]);
/// Service logo: a small quota ring (track at 25 % drawn separately).
pub const LOGO_TRACK: Icon = stroked((18.0, 18.0), 3.0, true, false, &[Circle(9.0, 9.0, 6.5)]);
pub const LOGO_ARC: Icon = stroked((18.0, 18.0), 3.0, true, false, &[P("M9 2.5a6.5 6.5 0 1 1-6.2 8.5")]);
pub const CHECK: Icon = stroked((10.0, 10.0), 2.0, true, true, &[P("M1.8 5.2l2.2 2.2 4.2-4.6")]);
