//! The four sheets. Each one is a list of coordinates and what sits at them;
//! the shapes come from [`crate::svg`].

use crate::svg::{Anchor, BG, Label, STROKE, Sheet, Side};

/// The fans' bus: GP4 arbitrates it, UART0 carries it, and the two fans share
/// one pair.
pub fn fans() -> Sheet {
    let (w, h) = (1180.0, 620.0);
    let mut s = Sheet::new(
        w,
        h,
        "The fans' RS-485 bus, pin by pin",
        "Raspberry Pi Pico W pins 36, 38, 6, 16 and 17 wired to a MAX485 module, and the module's \
         A and B pair daisy chained through fan 1 to fan 2, terminated at both ends.",
        1.0,
    );

    let (y_vcc, y_gnd, y_de, y_re, y_di, y_ro) = (140.0, 180.0, 235.0, 270.0, 320.0, 360.0);
    let (y_a, y_b, y_com) = (440.0, 480.0, 535.0);
    let y_gp4 = (y_de + y_re) / 2.0;

    let (px, py, pw, ph) = (40.0, 80.0, 200.0, 320.0); // Pico
    let (tx, ty, tw, th) = (400.0, 80.0, 190.0, 430.0); // MAX485
    let (f1x, f1y, fw, fh) = (730.0, 405.0, 170.0, 165.0); // fan 1
    let f2x = 970.0; // fan 2
    let (pr, tl, tr, f1l, f1r, f2l) = (px + pw, tx, tx + tw, f1x, f1x + fw, f2x);
    let branch = 320.0; // where GP4 splits to DE and RE
    let ground = 300.0; // where the ground goes on to the fans' common

    s.device(px, py, pw, ph, "Raspberry Pi Pico W", None);
    s.device(tx, ty, tw, th, "MAX485 module", Some("on 3V3(OUT)"));
    s.device(f1x, f1y, fw, fh, "Fan 1 · address 0x02", None);
    s.device(f2x, f1y, fw, fh, "Fan 2 · address 0x03", None);

    for (y, label) in [
        (y_vcc, "36 3V3(OUT)"),
        (y_gnd, "38 GND"),
        (y_gp4, "6 GP4"),
        (y_di, "16 GP12"),
        (y_ro, "17 GP13"),
    ] {
        s.pin(pr, y, label, Side::Right);
    }
    for (y, label) in [
        (y_vcc, "VCC"),
        (y_gnd, "GND"),
        (y_de, "DE"),
        (y_re, "RE"),
        (y_di, "DI"),
        (y_ro, "RO"),
    ] {
        s.pin(tl, y, label, Side::Left);
    }
    s.pin(tr, y_a, "A", Side::Right);
    s.pin(tr, y_b, "B", Side::Right);
    for (x, side) in [(f1l, Side::Left), (f1r, Side::Right), (f2l, Side::Left)] {
        for (y, label) in [(y_a, "A"), (y_b, "B"), (y_com, "common")] {
            s.pin(x, y, label, side);
        }
    }

    // The supply and the two data lines
    s.wire(&[(pr, y_vcc), (tl, y_vcc)], false);
    s.wire(&[(pr, y_gnd), (tl, y_gnd)], false);
    s.wire(&[(pr, y_di), (tl, y_di)], true);
    s.wire(&[(tl, y_ro), (pr, y_ro)], true);

    // GP4 branching to DE and RE
    s.wire(&[(pr, y_gp4), (branch, y_gp4)], false);
    s.wire(&[(branch, y_de), (branch, y_re)], false);
    s.wire(&[(branch, y_de), (tl, y_de)], true);
    s.wire(&[(branch, y_re), (tl, y_re)], true);
    s.dot(branch, y_gp4);
    s.brace(tl + 46.0, y_de - 12.0, y_re + 12.0, "tied together");

    // Ground carried on to the fans' RS-485 common
    s.wire(&[(pr, y_gnd), (ground, y_gnd)], false);
    s.rail(ground, y_gnd, y_com, &[y_gp4, y_di, y_ro]);
    s.wire(&[(ground, y_com), (f1l, y_com)], false);
    s.dot(ground, y_gnd);
    s.wire(&[(f1r, y_com), (f2l, y_com)], false);

    // The pair, daisy chained rather than run back to the transceiver
    for y in [y_a, y_b] {
        s.wire(&[(tr, y), (f1l, y)], false);
        s.wire(&[(f1r, y), (f2l, y)], false);
    }
    s.resistor(660.0, y_a, y_b, "120 Ω", Label::Right);
    s.resistor(935.0, y_a, y_b, "120 Ω", Label::Below);
    s.note(660.0, y_a - 55.0, "terminated here…", Anchor::Middle);
    s.note(935.0, y_a - 55.0, "…and at the last fan", Anchor::Middle);
    s.note(
        f1x + fw / 2.0,
        f1y + fh - 16.0,
        "mains: its own supply",
        Anchor::Middle,
    );
    s.note(
        f2x + fw / 2.0,
        f1y + fh - 16.0,
        "mains: its own supply",
        Anchor::Middle,
    );

    s.note(
        px,
        h - 40.0,
        "Every line is a wire. Arrows show which end drives it; the pair and the supply lines have no direction.",
        Anchor::Start,
    );
    s.note(
        px,
        h - 20.0,
        "GP4 drives DE and RE as one pin: low leaves the line to the fans, high takes it for the length of a request.",
        Anchor::Start,
    );
    s
}

/// The relay's bus: the same shape on UART1, one device on it, and a supply of
/// its own.
pub fn relay() -> Sheet {
    let (w, h) = (1340.0, 790.0);
    let mut s = Sheet::new(
        w,
        h,
        "The relay module's RS-485 bus, pin by pin",
        "Raspberry Pi Pico W pins 36, 38, 10, 11 and 12 wired to a second MAX485 module, and that \
         module's A and B pair to an LC-Modbus-1R-D7 relay module powered from its own 7 to 24 V supply.",
        1.0,
    );

    let (y_vcc, y_gnd, y_de, y_re, y_di, y_ro) = (140.0, 180.0, 235.0, 270.0, 320.0, 360.0);
    let (y_a, y_b, y_module_gnd, y_module_vcc) = (440.0, 480.0, 535.0, 575.0);
    let (y_contacts, y_opto) = (470.0, 545.0);
    let y_gp7 = (y_de + y_re) / 2.0;

    let (px, py, pw, ph) = (40.0, 80.0, 200.0, 320.0);
    let (tx, ty, tw, th) = (400.0, 80.0, 190.0, 430.0);
    let (rx, ry, rw, rh) = (860.0, 405.0, 220.0, 200.0);
    let (sx, sy, sw, sh) = (380.0, 640.0, 230.0, 95.0);
    let (pr, tl, tr, rl, rr) = (px + pw, tx, tx + tw, rx, rx + rw);
    let branch = 320.0;
    let ground = 300.0;

    s.device(px, py, pw, ph, "Raspberry Pi Pico W", None);
    s.device(tx, ty, tw, th, "MAX485 module", Some("on 3V3(OUT)"));
    s.device(rx, ry, rw, rh, "LC-Modbus-1R-D7", Some("address 0xFF"));
    s.device(
        sx,
        sy,
        sw,
        sh,
        "Its own 7–24 V supply",
        Some("never the Pico's VSYS"),
    );

    for (y, label) in [
        (y_vcc, "36 3V3(OUT)"),
        (y_gnd, "38 GND"),
        (y_gp7, "10 GP7"),
        (y_di, "11 GP8"),
        (y_ro, "12 GP9"),
    ] {
        s.pin(pr, y, label, Side::Right);
    }
    for (y, label) in [
        (y_vcc, "VCC"),
        (y_gnd, "GND"),
        (y_de, "DE"),
        (y_re, "RE"),
        (y_di, "DI"),
        (y_ro, "RO"),
    ] {
        s.pin(tl, y, label, Side::Left);
    }
    s.pin(tr, y_a, "A", Side::Right);
    s.pin(tr, y_b, "B", Side::Right);
    for (y, label) in [
        (y_a, "A"),
        (y_b, "B"),
        (y_module_gnd, "GND"),
        (y_module_vcc, "VCC"),
    ] {
        s.pin(rl, y, label, Side::Left);
    }
    s.pin(rr, y_contacts, "contacts", Side::Right);
    s.pin(rr, y_opto, "opto in", Side::Right);

    s.wire(&[(pr, y_vcc), (tl, y_vcc)], false);
    s.wire(&[(pr, y_gnd), (tl, y_gnd)], false);
    s.wire(&[(pr, y_di), (tl, y_di)], true);
    s.wire(&[(tl, y_ro), (pr, y_ro)], true);

    s.wire(&[(pr, y_gp7), (branch, y_gp7)], false);
    s.wire(&[(branch, y_de), (branch, y_re)], false);
    s.wire(&[(branch, y_de), (tl, y_de)], true);
    s.wire(&[(branch, y_re), (tl, y_re)], true);
    s.dot(branch, y_gp7);
    s.brace(tl + 46.0, y_de - 12.0, y_re + 12.0, "tied together");

    // Ground: the Pico's, the module's, and the supply's negative are one net
    s.wire(&[(pr, y_gnd), (ground, y_gnd)], false);
    s.rail(ground, y_gnd, y_module_gnd, &[y_gp7, y_di, y_ro]);
    s.wire(&[(ground, y_module_gnd), (rl, y_module_gnd)], false);
    s.dot(ground, y_gnd);
    s.wire(
        &[(ground, y_module_gnd), (ground, sy + 60.0), (sx, sy + 60.0)],
        false,
    );
    s.dot(ground, y_module_gnd);

    // The supply's positive, up and across to the module's VCC
    s.wire(
        &[
            (sx + sw - 50.0, sy),
            (sx + sw - 50.0, y_module_vcc),
            (rl, y_module_vcc),
        ],
        false,
    );
    s.note(sx + sw - 44.0, sy - 8.0, "+", Anchor::Start);
    s.note(sx + 6.0, sy + 54.0, "−", Anchor::Start);

    for y in [y_a, y_b] {
        s.wire(&[(tr, y), (rl, y)], false);
    }
    s.resistor(650.0, y_a, y_b, "120 Ω", Label::Right);
    s.resistor(820.0, y_a, y_b, "120 Ω", Label::Below);
    s.note(650.0, y_a - 45.0, "at the transceiver…", Anchor::Middle);
    s.note(820.0, y_a - 45.0, "…and at the module", Anchor::Middle);

    s.wire(&[(rr, y_contacts), (rr + 60.0, y_contacts)], false);
    s.note(
        rr + 70.0,
        y_contacts + 4.0,
        "the load being switched",
        Anchor::Start,
    );
    s.wire(&[(rr, y_opto), (rr + 60.0, y_opto)], false);
    s.note(rr + 70.0, y_opto + 4.0, "not connected", Anchor::Start);

    s.note(
        px,
        h - 40.0,
        "One device on this bus, at 9600 baud 8N1, which is why it is a second UART rather than two more addresses on the fans'.",
        Anchor::Start,
    );
    s.note(
        px,
        h - 20.0,
        "The module's GND is shared with the controller so the pair has a reference; its VCC is not.",
        Anchor::Start,
    );
    s
}

/// The button and the status LEDs: no transceiver, and nothing shared but
/// ground.
pub fn panel() -> Sheet {
    let (w, h) = (900.0, 480.0);
    let mut s = Sheet::new(
        w,
        h,
        "The button and the status LEDs, pin by pin",
        "Raspberry Pi Pico W pins 27, 26 and 24 to two LEDs through series resistors and to a \
         momentary button, all returning to pin 38, GND.",
        1.0,
    );

    let (y_led1, y_led2, y_button, y_gnd) = (120.0, 190.0, 275.0, 355.0);
    let (px, py, pw, ph) = (40.0, 70.0, 200.0, 330.0);
    let pr = px + pw;
    let rail = 700.0;

    s.device(px, py, pw, ph, "Raspberry Pi Pico W", None);
    for (y, label) in [
        (y_led1, "27 GP21"),
        (y_led2, "26 GP20"),
        (y_button, "24 GP18"),
        (y_gnd, "38 GND"),
    ] {
        s.pin(pr, y, label, Side::Right);
    }

    for (y, which) in [(y_led1, "LED 1, fan 1"), (y_led2, "LED 2, fan 2")] {
        s.wire(&[(pr, y), (rail, y)], false);
        s.resistor_inline(400.0, y, "330 Ω");
        s.led(540.0, y, which);
    }

    // The button: a gap in the wire, which is all a momentary switch is
    s.wire(&[(pr, y_button), (455.0, y_button)], false);
    s.wire(&[(495.0, y_button), (rail, y_button)], false);
    s.add(format!(
        r#"<path d="M455 {y_button} L497 {}" stroke="{STROKE}" stroke-width="1.6"/>"#,
        y_button - 22.0
    ));
    for x in [455, 495] {
        s.add(format!(
            r#"<circle cx="{x}" cy="{y_button}" r="3.4" fill="{BG}" stroke="{STROKE}" stroke-width="1.5"/>"#
        ));
    }
    s.note(475.0, y_button + 26.0, "button, momentary", Anchor::Middle);

    s.wire(&[(pr, y_gnd), (rail, y_gnd)], false);
    s.wire(&[(rail, y_led1), (rail, y_gnd)], false);
    for y in [y_led1, y_led2, y_button] {
        s.dot(rail, y);
    }

    s.note(
        rail + 14.0,
        y_gnd - 40.0,
        "one ground net, back to pin 38",
        Anchor::Start,
    );
    s.note(
        px,
        h - 40.0,
        "GP18 has the internal pull-up on and the firmware acts on the falling edge, so the switch needs no external resistor.",
        Anchor::Start,
    );
    s.note(
        px,
        h - 20.0,
        "Both LED outputs are active high: the pin drives the anode, the cathode goes to ground.",
        Anchor::Start,
    );
    s
}

/// Everything on one sheet: both buses, the panel, and the one ground net.
///
/// Set in larger type than the other three, because it is wide enough that a
/// page will scale it down to fit.
pub fn overview() -> Sheet {
    let (w, h) = (1620.0, 1600.0);
    let mut s = Sheet::new(
        w,
        h,
        "The whole controller, pin by pin",
        "Every wire of the fan controller on one sheet: the Raspberry Pi Pico W, the two MAX485 \
         modules on their own UARTs, the two fans, the relay module with its own supply, the \
         button and the two status LEDs, and the ground net all of it shares.",
        1.25,
    );

    let (y_3v3, y_gnd) = (215.0, 250.0);

    // The fans' bus, and the pair on to the two fans
    let (y_fde, y_fgp4, y_fre) = (330.0, 347.0, 365.0);
    let (y_fdi, y_fro, y_fvcc, y_fgnd) = (405.0, 445.0, 485.0, 520.0);
    let (y_a, y_b, y_com) = (560.0, 595.0, 650.0);

    // The relay's bus, the module, and its supply
    let (y_rde, y_rgp7, y_rre) = (780.0, 797.0, 815.0);
    let (y_rdi, y_rro, y_rvcc, y_rgnd) = (855.0, 895.0, 935.0, 970.0);
    let (y_ra, y_rb) = (1010.0, 1045.0);
    let (y_contacts, y_module_gnd, y_module_vcc) = (1060.0, 1100.0, 1140.0);

    // The panel, on the width left free under the two buses
    let (y_led1, y_led2, y_button, y_panel_return) = (1235.0, 1305.0, 1375.0, 1425.0);

    let (px, py, pw, ph) = (60.0, 150.0, 220.0, 1310.0);
    let pr = px + pw;
    let (rail_3v3, rail_gnd, branch) = (340.0, 390.0, 470.0);
    let panel_rail = 900.0;

    let (tfx, tfy, tfw, tfh) = (560.0, 250.0, 180.0, 370.0);
    let (trx, try_, trw, trh) = (560.0, 700.0, 180.0, 370.0);
    let (f1x, f2x, fy, fw, fh) = (1020.0, 1270.0, 520.0, 170.0, 175.0);
    let (rlx, rly, rlw, rlh) = (1020.0, 940.0, 215.0, 240.0);
    let (sx, sy, sw, sh) = (1320.0, 1100.0, 230.0, 95.0);

    s.device(px, py, pw, ph, "Raspberry Pi Pico W", None);
    s.device(tfx, tfy, tfw, tfh, "MAX485 module", None);
    s.device(trx, try_, trw, trh, "MAX485 module", None);
    s.device(f1x, fy, fw, fh, "Fan 1 · 0x02", None);
    s.device(f2x, fy, fw, fh, "Fan 2 · 0x03", None);
    s.device(rlx, rly, rlw, rlh, "LC-Modbus-1R-D7", None);
    s.note(
        rlx + rlw / 2.0,
        rly + rlh + 22.0,
        "address 0xFF",
        Anchor::Middle,
    );
    s.device(
        sx,
        sy,
        sw,
        sh,
        "Its own 7–24 V supply",
        Some("never the Pico's VSYS"),
    );

    for (y, label) in [
        (y_3v3, "36 3V3(OUT)"),
        (y_gnd, "38 GND"),
        (y_fgp4, "6 GP4"),
        (y_fdi, "16 GP12"),
        (y_fro, "17 GP13"),
        (y_rgp7, "10 GP7"),
        (y_rdi, "11 GP8"),
        (y_rro, "12 GP9"),
        (y_led1, "27 GP21"),
        (y_led2, "26 GP20"),
        (y_button, "24 GP18"),
    ] {
        s.pin(pr, y, label, Side::Right);
    }

    for (y, label) in [
        (y_fde, "DE"),
        (y_fre, "RE"),
        (y_fdi, "DI"),
        (y_fro, "RO"),
        (y_fvcc, "VCC"),
        (y_fgnd, "GND"),
    ] {
        s.pin(tfx, y, label, Side::Left);
    }
    s.pin(tfx + tfw, y_a, "A", Side::Right);
    s.pin(tfx + tfw, y_b, "B", Side::Right);

    for (y, label) in [
        (y_rde, "DE"),
        (y_rre, "RE"),
        (y_rdi, "DI"),
        (y_rro, "RO"),
        (y_rvcc, "VCC"),
        (y_rgnd, "GND"),
    ] {
        s.pin(trx, y, label, Side::Left);
    }
    s.pin(trx + trw, y_ra, "A", Side::Right);
    s.pin(trx + trw, y_rb, "B", Side::Right);

    for (x, side) in [
        (f1x, Side::Left),
        (f1x + fw, Side::Right),
        (f2x, Side::Left),
    ] {
        for (y, label) in [(y_a, "A"), (y_b, "B"), (y_com, "common")] {
            s.pin(x, y, label, side);
        }
    }

    for (y, label) in [(y_ra, "A"), (y_rb, "B"), (y_module_gnd, "GND")] {
        s.pin(rlx, y, label, Side::Left);
    }
    s.pin(rlx + rlw, y_ra, "opto in", Side::Right);
    s.pin(rlx + rlw, y_contacts, "contacts", Side::Right);
    s.pin(rlx + rlw, y_module_vcc, "VCC", Side::Right);

    let signals = [
        y_fde, y_fgp4, y_fre, y_fdi, y_fro, y_rde, y_rgp7, y_rre, y_rdi, y_rro, y_led1, y_led2,
        y_button,
    ];
    let crossing = |from: f64, to: f64| -> Vec<f64> {
        signals
            .iter()
            .copied()
            .filter(|y| from < *y && *y < to)
            .collect()
    };

    // The two rails everything else hangs off, hopping the signals they cross
    s.wire(&[(pr, y_3v3), (rail_3v3, y_3v3)], false);
    s.rail(rail_3v3, y_3v3, y_rvcc, &crossing(y_3v3, y_rvcc));
    s.wire(&[(rail_3v3, y_fvcc), (tfx, y_fvcc)], false);
    s.wire(&[(rail_3v3, y_rvcc), (trx, y_rvcc)], false);
    s.dot(rail_3v3, y_fvcc);
    s.note(
        rail_3v3 + 10.0,
        y_3v3 - 12.0,
        "3V3(OUT) rail",
        Anchor::Start,
    );

    s.wire(&[(pr, y_gnd), (rail_gnd, y_gnd)], false);
    s.rail(
        rail_gnd,
        y_gnd,
        y_panel_return,
        &crossing(y_gnd, y_panel_return),
    );
    for (y, to) in [
        (y_fgnd, tfx),
        (y_com, f1x),
        (y_rgnd, trx),
        (y_module_gnd, rlx),
        (y_panel_return, panel_rail),
    ] {
        s.wire(&[(rail_gnd, y), (to, y)], false);
        s.dot(rail_gnd, y);
    }
    s.note(rail_gnd + 10.0, y_gnd + 26.0, "ground rail", Anchor::Start);

    // The fans' bus
    s.wire(&[(pr, y_fdi), (tfx, y_fdi)], true);
    s.wire(&[(tfx, y_fro), (pr, y_fro)], true);
    s.wire(&[(pr, y_fgp4), (branch, y_fgp4)], false);
    s.wire(&[(branch, y_fde), (branch, y_fre)], false);
    s.wire(&[(branch, y_fde), (tfx, y_fde)], true);
    s.wire(&[(branch, y_fre), (tfx, y_fre)], true);
    s.dot(branch, y_fgp4);
    s.brace(tfx + 52.0, y_fde - 12.0, y_fre + 12.0, "tied together");

    for y in [y_a, y_b] {
        s.wire(&[(tfx + tfw, y), (f1x, y)], false);
        s.wire(&[(f1x + fw, y), (f2x, y)], false);
    }
    s.wire(&[(f1x + fw, y_com), (f2x, y_com)], false);
    s.resistor(830.0, y_a, y_b, "120 Ω", Label::Right);
    s.resistor(1225.0, y_a, y_b, "120 Ω", Label::Below);
    s.note(
        f1x + fw / 2.0,
        fy + fh - 18.0,
        "mains: its own supply",
        Anchor::Middle,
    );
    s.note(
        f2x + fw / 2.0,
        fy + fh - 18.0,
        "mains: its own supply",
        Anchor::Middle,
    );

    // The relay's bus
    s.wire(&[(pr, y_rdi), (trx, y_rdi)], true);
    s.wire(&[(trx, y_rro), (pr, y_rro)], true);
    s.wire(&[(pr, y_rgp7), (branch, y_rgp7)], false);
    s.wire(&[(branch, y_rde), (branch, y_rre)], false);
    s.wire(&[(branch, y_rde), (trx, y_rde)], true);
    s.wire(&[(branch, y_rre), (trx, y_rre)], true);
    s.dot(branch, y_rgp7);
    s.brace(trx + 52.0, y_rde - 12.0, y_rre + 12.0, "tied together");

    for y in [y_ra, y_rb] {
        s.wire(&[(trx + trw, y), (rlx, y)], false);
    }
    s.resistor(830.0, y_ra, y_rb, "120 Ω", Label::Right);
    s.resistor(985.0, y_ra, y_rb, "120 Ω", Label::Below);

    // The relay module's own supply. Only the ground is shared with the controller
    s.wire(&[(rlx + rlw, y_module_vcc), (sx, y_module_vcc)], false);
    s.note(sx - 12.0, sy - 10.0, "+", Anchor::End);
    s.wire(
        &[
            (sx, sy + 75.0),
            (sx - 60.0, sy + 75.0),
            (sx - 60.0, 1345.0),
            (960.0, 1345.0),
            (960.0, y_module_gnd),
        ],
        false,
    );
    s.dot(960.0, y_module_gnd);
    s.note(sx - 12.0, sy + 70.0, "−", Anchor::End);
    s.wire(&[(rlx + rlw, y_ra), (rlx + rlw + 60.0, y_ra)], false);
    s.note(rlx + rlw + 70.0, y_ra + 5.0, "not connected", Anchor::Start);
    s.wire(
        &[(rlx + rlw, y_contacts), (rlx + rlw + 60.0, y_contacts)],
        false,
    );
    s.note(
        rlx + rlw + 70.0,
        y_contacts + 5.0,
        "the load being switched",
        Anchor::Start,
    );

    // The panel
    for (y, which) in [(y_led1, "LED 1, fan 1"), (y_led2, "LED 2, fan 2")] {
        s.wire(&[(pr, y), (panel_rail, y)], false);
        s.resistor_inline(560.0, y, "330 Ω");
        s.led(700.0, y, which);
    }
    s.wire(&[(pr, y_button), (610.0, y_button)], false);
    s.wire(&[(650.0, y_button), (panel_rail, y_button)], false);
    s.add(format!(
        r#"<path d="M610 {y_button} L652 {}" stroke="{STROKE}" stroke-width="1.6"/>"#,
        y_button - 22.0
    ));
    for x in [610, 650] {
        s.add(format!(
            r#"<circle cx="{x}" cy="{y_button}" r="3.4" fill="{BG}" stroke="{STROKE}" stroke-width="1.5"/>"#
        ));
    }
    s.note(630.0, y_button + 28.0, "button, momentary", Anchor::Middle);
    s.wire(&[(panel_rail, y_led1), (panel_rail, y_panel_return)], false);
    for y in [y_led1, y_led2, y_button] {
        s.dot(panel_rail, y);
    }

    s.note(
        px,
        h - 58.0,
        "One sheet, two buses. The fans answer 19 200 baud 8E1 on UART0, the relay 9600 8N1 on UART1, which is why it has a transceiver of its own rather than an address on the fans' bus.",
        Anchor::Start,
    );
    s.note(
        px,
        h - 33.0,
        "Ground is a single net: both transceivers, the LED cathodes, the button, the fans' RS-485 common and the relay module's supply return all meet at pin 38.",
        Anchor::Start,
    );
    s
}
