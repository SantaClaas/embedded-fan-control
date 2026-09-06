#!/usr/bin/env python3
"""Draws the wiring diagrams next to this file.

    cd fan-controller/documentation && python3 wiring.py

Nothing else in the repository runs this; it exists so a wire can be moved by
editing coordinates rather than SVG paths. The output is checked in, so run it
and commit the result. Standard library only, no dependencies.

Conventions the drawings keep to: a filled dot is a junction, a hop is a
crossing that is not one, an arrow marks the end that drives a line, and supply
and differential-pair lines carry no arrow.
"""

BG      = "#ffffff"
BOX_F   = "#f6f6f3"
BOX_S   = "#3a3a3a"
WIRE    = "#3a3a3a"
TEXT    = "#1a1a1a"
MUTED   = "#5c5c5c"
FONT    = "ui-sans-serif, -apple-system, 'Segoe UI', Helvetica, Arial, sans-serif"
MONO    = "ui-monospace, SFMono-Regular, Menlo, Consolas, monospace"


class Svg:
    def __init__(self, w, h, title, desc, fs=1.0):
        self.w, self.h = w, h
        self.o = []
        self.title, self.desc = title, desc
        self.fs = fs                      # font scale, for a sheet that gets shrunk to fit

    def add(self, s):
        self.o.append("  " + s)

    def box(self, x, y, w, h, title, subtitle=None):
        self.add(f'<rect x="{x}" y="{y}" width="{w}" height="{h}" rx="6" '
                 f'fill="{BOX_F}" stroke="{BOX_S}" stroke-width="1.5"/>')
        self.add(f'<text x="{x + w / 2}" y="{y + 24}" text-anchor="middle" '
                 f'font-family="{FONT}" font-size="{15 * self.fs:.1f}" font-weight="600" fill="{TEXT}">{title}</text>')
        if subtitle:
            self.add(f'<text x="{x + w / 2}" y="{y + 43}" text-anchor="middle" '
                     f'font-family="{FONT}" font-size="{12.5 * self.fs:.1f}" fill="{MUTED}">{subtitle}</text>')

    def pin(self, x, y, label, side):
        """Pin label just inside a box edge. side is 'l' or 'r'."""
        dx, anchor = (8, "start") if side == "l" else (-8, "end")
        self.add(f'<text x="{x + dx}" y="{y + 4.5}" text-anchor="{anchor}" '
                 f'font-family="{MONO}" font-size="{12.5 * self.fs:.1f}" fill="{TEXT}">{label}</text>')

    def wire(self, points, arrow=False, dash=False):
        d = " ".join(("M" if i == 0 else "L") + f"{p[0]} {p[1]}" for i, p in enumerate(points))
        a = ' marker-end="url(#arrow)"' if arrow else ""
        s = ' stroke-dasharray="5 4"' if dash else ""
        self.add(f'<path d="{d}" fill="none" stroke="{WIRE}" stroke-width="1.6" '
                 f'stroke-linejoin="round"{s}{a}/>')

    def dot(self, x, y):
        self.add(f'<circle cx="{x}" cy="{y}" r="3.4" fill="{WIRE}"/>')

    def resistor(self, x, y1, y2, label, where="right"):
        """A resistor bridging two horizontal wires at column x."""
        mid = (y1 + y2) / 2
        self.wire([(x, y1), (x, mid - 15)])
        self.wire([(x, mid + 15), (x, y2)])
        self.add(f'<rect x="{x - 8}" y="{mid - 15}" width="16" height="30" rx="2" '
                 f'fill="{BG}" stroke="{WIRE}" stroke-width="1.5"/>')
        if where == "right":
            self.add(f'<text x="{x + 15}" y="{mid + 4.5}" font-family="{MONO}" font-size="{12.5 * self.fs:.1f}" '
                     f'fill="{TEXT}">{label}</text>')
        else:
            self.add(f'<text x="{x}" y="{y2 + 26}" text-anchor="middle" font-family="{MONO}" '
                     f'font-size="{12.5 * self.fs:.1f}" fill="{TEXT}">{label}</text>')
        self.dot(x, y1)
        self.dot(x, y2)

    def vwire_hopping(self, x, y1, y2, crossings):
        """A vertical wire that hops over the horizontals it crosses rather than touching them."""
        d = [f"M{x} {y1}"]
        for cy in sorted(crossings):
            d.append(f"L{x} {cy - 6}")
            d.append(f"A 6 6 0 0 0 {x} {cy + 6}")
        d.append(f"L{x} {y2}")
        self.add(f'<path d="{" ".join(d)}" fill="none" stroke="{WIRE}" stroke-width="1.6"/>')

    def note(self, x, y, text, anchor="start", size=12.5):
        self.add(f'<text x="{x}" y="{y}" text-anchor="{anchor}" font-family="{FONT}" '
                 f'font-size="{size * self.fs:.1f}" fill="{MUTED}">{text}</text>')

    def brace(self, x, y1, y2, text):
        """A curly-ish brace tying two rows together, with a note."""
        self.add(f'<path d="M{x} {y1} L{x + 8} {y1} L{x + 8} {y2} L{x} {y2}" fill="none" '
                 f'stroke="{MUTED}" stroke-width="1.2"/>')
        self.note(x + 14, (y1 + y2) / 2 + 4.5, text)

    def render(self):
        head = (f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {self.w} {self.h}" '
                f'width="{self.w}" height="{self.h}" role="img" aria-label="{self.title}">\n'
                f'  <title>{self.title}</title>\n  <desc>{self.desc}</desc>\n'
                f'  <defs>\n'
                f'    <marker id="arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7" '
                f'markerHeight="7" orient="auto-start-reverse">\n'
                f'      <path d="M0 0 L10 5 L0 10 z" fill="{WIRE}"/>\n'
                f'    </marker>\n  </defs>\n'
                f'  <rect width="{self.w}" height="{self.h}" fill="{BG}"/>')
        return head + "\n" + "\n".join(self.o) + "\n</svg>\n"

    def res_inline(self, x, y, label):
        """A resistor in line with a horizontal wire, centred on x."""
        self.add(f'<rect x="{x - 19}" y="{y - 8}" width="38" height="16" rx="2" '
                 f'fill="{BG}" stroke="{WIRE}" stroke-width="1.5"/>')
        self.add(f'<text x="{x}" y="{y - 15}" text-anchor="middle" font-family="{MONO}" '
                 f'font-size="{12.5 * self.fs:.1f}" fill="{TEXT}">{label}</text>')

    def led(self, x, y, label):
        """An LED in line with a horizontal wire, anode on the left."""
        self.add(f'<path d="M{x - 9} {y - 10} L{x + 8} {y} L{x - 9} {y + 10} z" '
                 f'fill="{BOX_F}" stroke="{WIRE}" stroke-width="1.5" stroke-linejoin="round"/>')
        self.add(f'<path d="M{x + 8} {y - 11} L{x + 8} {y + 11}" stroke="{WIRE}" stroke-width="1.8"/>')
        self.add(f'<path d="M{x + 2} {y - 14} l7 -7 M{x + 8} {y - 18} l7 -7" stroke="{WIRE}" '
                 f'stroke-width="1.2" marker-end="url(#arrow)"/>')
        self.add(f'<text x="{x}" y="{y + 30}" text-anchor="middle" font-family="{FONT}" '
                 f'font-size="{12.5 * self.fs:.1f}" fill="{MUTED}">{label}</text>')


def overview():
    """Everything on one sheet: both buses, the panel, and the one ground net."""
    W, H = 1620, 1600
    s = Svg(W, H, "The whole controller, pin by pin",
            "Every wire of the fan controller on one sheet: the Raspberry Pi Pico W, the two MAX485 "
            "modules on their own UARTs, the two fans, the relay module with its own supply, the "
            "button and the two status LEDs, and the ground net all of it shares.", fs=1.25)

    Y_3V3, Y_GND = 215, 250

    # The fans' bus, and the pair on to the two fans
    Y_FDE, Y_FGP4, Y_FRE = 330, 347, 365
    Y_FDI, Y_FRO, Y_FVCC, Y_FGND = 405, 445, 485, 520
    Y_A, Y_B, Y_COM = 560, 595, 650

    # The relay's bus, the module, and its supply
    Y_RDE, Y_RGP7, Y_RRE = 780, 797, 815
    Y_RDI, Y_RRO, Y_RVCC, Y_RGND = 855, 895, 935, 970
    Y_RA, Y_RB = 1010, 1045
    Y_CONTACTS, Y_MGND, Y_MVCC = 1060, 1100, 1140

    # The panel, on the width left free under the two buses
    Y_L1, Y_L2, Y_BTN, Y_PRET = 1235, 1305, 1375, 1425

    PX, PY, PW, PH = 60, 150, 220, 1310
    PR = PX + PW
    V3, VG, BR = 340, 390, 470            # the 3V3 rail, the ground rail, the DE/RE branch
    PANEL_RAIL = 900

    TFX, TFY, TFW, TFH = 560, 250, 180, 370
    TRX, TRY, TRW, TRH = 560, 700, 180, 370
    F1X, F2X, FY, FW, FH = 1020, 1270, 520, 170, 175
    RLX, RLY, RLW, RLH = 1020, 940, 215, 240
    SX, SY, SW, SH = 1320, 1100, 230, 95

    s.box(PX, PY, PW, PH, "Raspberry Pi Pico W")
    s.box(TFX, TFY, TFW, TFH, "MAX485 module")
    s.box(TRX, TRY, TRW, TRH, "MAX485 module")
    s.box(F1X, FY, FW, FH, "Fan 1 · 0x02")
    s.box(F2X, FY, FW, FH, "Fan 2 · 0x03")
    s.box(RLX, RLY, RLW, RLH, "LC-Modbus-1R-D7")
    s.note(RLX + RLW / 2, RLY + RLH + 22, "address 0xFF", anchor="middle")
    s.box(SX, SY, SW, SH, "Its own 7–24 V supply", "never the Pico's VSYS")

    for y, label in ((Y_3V3, "36 3V3(OUT)"), (Y_GND, "38 GND"),
                     (Y_FGP4, "6 GP4"), (Y_FDI, "16 GP12"), (Y_FRO, "17 GP13"),
                     (Y_RGP7, "10 GP7"), (Y_RDI, "11 GP8"), (Y_RRO, "12 GP9"),
                     (Y_L1, "27 GP21"), (Y_L2, "26 GP20"), (Y_BTN, "24 GP18")):
        s.pin(PR, y, label, "r")

    for y, label in ((Y_FDE, "DE"), (Y_FRE, "RE"), (Y_FDI, "DI"), (Y_FRO, "RO"),
                     (Y_FVCC, "VCC"), (Y_FGND, "GND")):
        s.pin(TFX, y, label, "l")
    s.pin(TFX + TFW, Y_A, "A", "r")
    s.pin(TFX + TFW, Y_B, "B", "r")

    for y, label in ((Y_RDE, "DE"), (Y_RRE, "RE"), (Y_RDI, "DI"), (Y_RRO, "RO"),
                     (Y_RVCC, "VCC"), (Y_RGND, "GND")):
        s.pin(TRX, y, label, "l")
    s.pin(TRX + TRW, Y_RA, "A", "r")
    s.pin(TRX + TRW, Y_RB, "B", "r")

    for x, side in ((F1X, "l"), (F1X + FW, "r")):
        for y, label in ((Y_A, "A"), (Y_B, "B"), (Y_COM, "common")):
            s.pin(x, y, label, side)
    for y, label in ((Y_A, "A"), (Y_B, "B"), (Y_COM, "common")):
        s.pin(F2X, y, label, "l")

    for y, label in ((Y_RA, "A"), (Y_RB, "B"), (Y_MGND, "GND")):
        s.pin(RLX, y, label, "l")
    s.pin(RLX + RLW, Y_RA, "opto in", "r")
    s.pin(RLX + RLW, Y_CONTACTS, "contacts", "r")
    s.pin(RLX + RLW, Y_MVCC, "VCC", "r")

    signals = [Y_FDE, Y_FGP4, Y_FRE, Y_FDI, Y_FRO,
               Y_RDE, Y_RGP7, Y_RRE, Y_RDI, Y_RRO, Y_L1, Y_L2, Y_BTN]

    # The two rails everything else hangs off, hopping the signals they cross
    s.wire([(PR, Y_3V3), (V3, Y_3V3)])
    s.vwire_hopping(V3, Y_3V3, Y_RVCC, [y for y in signals if Y_3V3 < y < Y_RVCC])
    s.wire([(V3, Y_FVCC), (TFX, Y_FVCC)])
    s.wire([(V3, Y_RVCC), (TRX, Y_RVCC)])
    s.dot(V3, Y_FVCC)
    s.note(V3 + 10, Y_3V3 - 12, "3V3(OUT) rail")

    s.wire([(PR, Y_GND), (VG, Y_GND)])
    s.vwire_hopping(VG, Y_GND, Y_PRET, [y for y in signals if Y_GND < y < Y_PRET])
    for y, x2 in ((Y_FGND, TFX), (Y_COM, F1X), (Y_RGND, TRX), (Y_MGND, RLX), (Y_PRET, PANEL_RAIL)):
        s.wire([(VG, y), (x2, y)])
        s.dot(VG, y)
    s.note(VG + 10, Y_GND + 26, "ground rail")

    # The fans' bus
    s.wire([(PR, Y_FDI), (TFX, Y_FDI)], arrow=True)
    s.wire([(TFX, Y_FRO), (PR, Y_FRO)], arrow=True)
    s.wire([(PR, Y_FGP4), (BR, Y_FGP4)])
    s.wire([(BR, Y_FDE), (BR, Y_FRE)])
    s.wire([(BR, Y_FDE), (TFX, Y_FDE)], arrow=True)
    s.wire([(BR, Y_FRE), (TFX, Y_FRE)], arrow=True)
    s.dot(BR, Y_FGP4)
    s.brace(TFX + 52, Y_FDE - 12, Y_FRE + 12, "tied together")

    for y in (Y_A, Y_B):
        s.wire([(TFX + TFW, y), (F1X, y)])
        s.wire([(F1X + FW, y), (F2X, y)])
    s.wire([(F1X + FW, Y_COM), (F2X, Y_COM)])
    s.resistor(830, Y_A, Y_B, "120 Ω")
    s.resistor(1225, Y_A, Y_B, "120 Ω", where="below")
    s.note(F1X + FW / 2, FY + FH - 18, "mains: its own supply", anchor="middle")
    s.note(F2X + FW / 2, FY + FH - 18, "mains: its own supply", anchor="middle")

    # The relay's bus
    s.wire([(PR, Y_RDI), (TRX, Y_RDI)], arrow=True)
    s.wire([(TRX, Y_RRO), (PR, Y_RRO)], arrow=True)
    s.wire([(PR, Y_RGP7), (BR, Y_RGP7)])
    s.wire([(BR, Y_RDE), (BR, Y_RRE)])
    s.wire([(BR, Y_RDE), (TRX, Y_RDE)], arrow=True)
    s.wire([(BR, Y_RRE), (TRX, Y_RRE)], arrow=True)
    s.dot(BR, Y_RGP7)
    s.brace(TRX + 52, Y_RDE - 12, Y_RRE + 12, "tied together")

    for y in (Y_RA, Y_RB):
        s.wire([(TRX + TRW, y), (RLX, y)])
    s.resistor(830, Y_RA, Y_RB, "120 Ω")
    s.resistor(985, Y_RA, Y_RB, "120 Ω", where="below")

    # The relay module's own supply. Only the ground is shared with the controller
    s.wire([(RLX + RLW, Y_MVCC), (SX, Y_MVCC)])
    s.note(SX - 12, SY - 10, "+", anchor="end")
    s.wire([(SX, SY + 75), (SX - 60, SY + 75), (SX - 60, 1345), (960, 1345), (960, Y_MGND)])
    s.dot(960, Y_MGND)
    s.note(SX - 12, SY + 70, "−", anchor="end")
    s.wire([(RLX + RLW, Y_RA), (RLX + RLW + 60, Y_RA)])
    s.note(RLX + RLW + 70, Y_RA + 5, "not connected")
    s.wire([(RLX + RLW, Y_CONTACTS), (RLX + RLW + 60, Y_CONTACTS)])
    s.note(RLX + RLW + 70, Y_CONTACTS + 5, "the load being switched")

    # The panel
    for y, which in ((Y_L1, "LED 1, fan 1"), (Y_L2, "LED 2, fan 2")):
        s.wire([(PR, y), (PANEL_RAIL, y)])
        s.res_inline(560, y, "330 Ω")
        s.led(700, y, which)
    s.wire([(PR, Y_BTN), (610, Y_BTN)])
    s.wire([(650, Y_BTN), (PANEL_RAIL, Y_BTN)])
    s.add(f'<path d="M610 {Y_BTN} L652 {Y_BTN - 22}" stroke="{WIRE}" stroke-width="1.6"/>')
    for x in (610, 650):
        s.add(f'<circle cx="{x}" cy="{Y_BTN}" r="3.4" fill="{BG}" stroke="{WIRE}" stroke-width="1.5"/>')
    s.note(630, Y_BTN + 28, "button, momentary", anchor="middle")
    s.wire([(PANEL_RAIL, Y_L1), (PANEL_RAIL, Y_PRET)])
    for y in (Y_L1, Y_L2, Y_BTN):
        s.dot(PANEL_RAIL, y)

    s.note(PX, H - 58, "One sheet, two buses. The fans answer 19 200 baud 8E1 on UART0, the relay "
                       "9600 8N1 on UART1, which is why it has a transceiver of its own rather "
                       "than an address on the fans' bus.")
    s.note(PX, H - 33, "Ground is a single net: both transceivers, the LED cathodes, the button, "
                       "the fans' RS-485 common and the relay module's supply return all meet at "
                       "pin 38.")
    return s


def fans():
    W, H = 1180, 620
    s = Svg(W, H, "The fans' RS-485 bus, pin by pin",
            "Raspberry Pi Pico W pins 36, 38, 6, 16 and 17 wired to a MAX485 module, and the module's "
            "A and B pair daisy chained through fan 1 to fan 2, terminated at both ends.")

    Y_VCC, Y_GND, Y_DE, Y_RE, Y_DI, Y_RO = 140, 180, 235, 270, 320, 360
    Y_A, Y_B, Y_COM = 440, 480, 535

    PX, PY, PW, PH = 40, 80, 200, 320           # Pico
    TX, TY, TW, TH = 400, 80, 190, 430          # MAX485
    F1X, F1Y, F1W, F1H = 730, 405, 170, 165     # fan 1
    F2X, F2Y, F2W, F2H = 970, 405, 170, 165     # fan 2
    PR, TL, TR, F1L, F1R, F2L = PX + PW, TX, TX + TW, F1X, F1X + F1W, F2X

    s.box(PX, PY, PW, PH, "Raspberry Pi Pico W")
    s.box(TX, TY, TW, TH, "MAX485 module", "on 3V3(OUT)")
    s.box(F1X, F1Y, F1W, F1H, "Fan 1 · address 0x02")
    s.box(F2X, F2Y, F2W, F2H, "Fan 2 · address 0x03")

    for y, label in ((Y_VCC, "36 3V3(OUT)"), (Y_GND, "38 GND"), (Y_DI, "16 GP12"), (Y_RO, "17 GP13")):
        s.pin(PR, y, label, "r")
    s.pin(PR, (Y_DE + Y_RE) / 2, "6 GP4", "r")

    for y, label in ((Y_VCC, "VCC"), (Y_GND, "GND"), (Y_DE, "DE"), (Y_RE, "RE"), (Y_DI, "DI"), (Y_RO, "RO")):
        s.pin(TL, y, label, "l")
    s.pin(TR, Y_A, "A", "r")
    s.pin(TR, Y_B, "B", "r")

    for x, side in ((F1L, "l"), (F1R, "r")):
        for y, label in ((Y_A, "A"), (Y_B, "B"), (Y_COM, "common")):
            s.pin(x, y, label, side)
    for y, label in ((Y_A, "A"), (Y_B, "B"), (Y_COM, "common")):
        s.pin(F2L, y, label, "l")

    # Supply and the two data lines
    s.wire([(PR, Y_VCC), (TL, Y_VCC)])
    s.wire([(PR, Y_GND), (TL, Y_GND)])
    s.wire([(PR, Y_DI), (TL, Y_DI)], arrow=True)
    s.wire([(TL, Y_RO), (PR, Y_RO)], arrow=True)

    # GP4 branching to DE and RE
    BR = 320
    s.wire([(PR, (Y_DE + Y_RE) / 2), (BR, (Y_DE + Y_RE) / 2)])
    s.wire([(BR, Y_DE), (BR, Y_RE)])
    s.wire([(BR, Y_DE), (TL, Y_DE)], arrow=True)
    s.wire([(BR, Y_RE), (TL, Y_RE)], arrow=True)
    s.dot(BR, (Y_DE + Y_RE) / 2)
    s.brace(TL + 46, Y_DE - 12, Y_RE + 12, "tied together")

    # Ground carried on to the fans' RS-485 common
    GB = 300
    s.wire([(PR, Y_GND), (GB, Y_GND)])
    s.vwire_hopping(GB, Y_GND, Y_COM, [(Y_DE + Y_RE) / 2, Y_DI, Y_RO])
    s.wire([(GB, Y_COM), (F1L, Y_COM)])
    s.dot(GB, Y_GND)
    s.wire([(F1R, Y_COM), (F2L, Y_COM)])

    # The pair, daisy chained
    for y in (Y_A, Y_B):
        s.wire([(TR, y), (F1L, y)])
        s.wire([(F1R, y), (F2L, y)])

    s.resistor(660, Y_A, Y_B, "120 Ω")
    s.resistor(935, Y_A, Y_B, "120 Ω", where="below")
    s.note(660, Y_A - 55, "terminated here…", anchor="middle")
    s.note(935, Y_A - 55, "…and at the last fan", anchor="middle")

    s.note(F1X + F1W / 2, F1Y + F1H - 16, "mains: its own supply", anchor="middle")
    s.note(F2X + F2W / 2, F2Y + F2H - 16, "mains: its own supply", anchor="middle")

    s.note(PX, H - 40, "Every line is a wire. Arrows show which end drives it; the pair and the "
                       "supply lines have no direction.")
    s.note(PX, H - 20, "GP4 drives DE and RE as one pin: low leaves the line to the fans, high takes "
                       "it for the length of a request.")

    return s


def relay():
    W, H = 1340, 790
    s = Svg(W, H, "The relay module's RS-485 bus, pin by pin",
            "Raspberry Pi Pico W pins 36, 38, 10, 11 and 12 wired to a second MAX485 module, and that "
            "module's A and B pair to an LC-Modbus-1R-D7 relay module powered from its own 7 to 24 V supply.")

    Y_VCC, Y_GND, Y_DE, Y_RE, Y_DI, Y_RO = 140, 180, 235, 270, 320, 360
    Y_A, Y_B, Y_MGND, Y_MVCC = 440, 480, 535, 575

    PX, PY, PW, PH = 40, 80, 200, 320
    TX, TY, TW, TH = 400, 80, 190, 430
    RX, RY, RW, RH = 860, 405, 220, 200
    SX, SY, SW, SH = 380, 640, 230, 95
    PR, TL, TR, RL, RR = PX + PW, TX, TX + TW, RX, RX + RW

    s.box(PX, PY, PW, PH, "Raspberry Pi Pico W")
    s.box(TX, TY, TW, TH, "MAX485 module", "on 3V3(OUT)")
    s.box(RX, RY, RW, RH, "LC-Modbus-1R-D7", "address 0xFF")
    s.box(SX, SY, SW, SH, "Its own 7–24 V supply", "never the Pico's VSYS")

    for y, label in ((Y_VCC, "36 3V3(OUT)"), (Y_GND, "38 GND"), (Y_DI, "11 GP8"), (Y_RO, "12 GP9")):
        s.pin(PR, y, label, "r")
    s.pin(PR, (Y_DE + Y_RE) / 2, "10 GP7", "r")

    for y, label in ((Y_VCC, "VCC"), (Y_GND, "GND"), (Y_DE, "DE"), (Y_RE, "RE"), (Y_DI, "DI"), (Y_RO, "RO")):
        s.pin(TL, y, label, "l")
    s.pin(TR, Y_A, "A", "r")
    s.pin(TR, Y_B, "B", "r")

    for y, label in ((Y_A, "A"), (Y_B, "B"), (Y_MGND, "GND"), (Y_MVCC, "VCC")):
        s.pin(RL, y, label, "l")
    s.pin(RR, 470, "contacts", "r")
    s.pin(RR, 545, "opto in", "r")

    s.wire([(PR, Y_VCC), (TL, Y_VCC)])
    s.wire([(PR, Y_GND), (TL, Y_GND)])
    s.wire([(PR, Y_DI), (TL, Y_DI)], arrow=True)
    s.wire([(TL, Y_RO), (PR, Y_RO)], arrow=True)

    BR = 320
    s.wire([(PR, (Y_DE + Y_RE) / 2), (BR, (Y_DE + Y_RE) / 2)])
    s.wire([(BR, Y_DE), (BR, Y_RE)])
    s.wire([(BR, Y_DE), (TL, Y_DE)], arrow=True)
    s.wire([(BR, Y_RE), (TL, Y_RE)], arrow=True)
    s.dot(BR, (Y_DE + Y_RE) / 2)
    s.brace(TL + 46, Y_DE - 12, Y_RE + 12, "tied together")

    # Ground: the Pico's, the module's, and the supply's negative are one net
    GB = 300
    s.wire([(PR, Y_GND), (GB, Y_GND)])
    s.vwire_hopping(GB, Y_GND, Y_MGND, [(Y_DE + Y_RE) / 2, Y_DI, Y_RO])
    s.wire([(GB, Y_MGND), (RL, Y_MGND)])
    s.dot(GB, Y_GND)
    s.wire([(GB, Y_MGND), (GB, SY + 60), (SX, SY + 60)])
    s.dot(GB, Y_MGND)

    # The supply's positive, up and across to the module's VCC
    s.wire([(SX + SW - 50, SY), (SX + SW - 50, Y_MVCC), (RL, Y_MVCC)])
    s.note(SX + SW - 44, SY - 8, "+")
    s.note(SX + 6, SY + 54, "−")

    for y in (Y_A, Y_B):
        s.wire([(TR, y), (RL, y)])
    s.resistor(650, Y_A, Y_B, "120 Ω")
    s.resistor(820, Y_A, Y_B, "120 Ω", where="below")
    s.note(650, Y_A - 45, "at the transceiver…", anchor="middle")
    s.note(820, Y_A - 45, "…and at the module", anchor="middle")

    s.wire([(RR, 470), (RR + 60, 470)])
    s.note(RR + 70, 474, "the load being switched")
    s.wire([(RR, 545), (RR + 60, 545)])
    s.note(RR + 70, 549, "not connected")

    s.note(PX, H - 40, "One device on this bus, at 9600 baud 8N1, which is why it is a second UART "
                       "rather than two more addresses on the fans'.")
    s.note(PX, H - 20, "The module's GND is shared with the controller so the pair has a reference; "
                       "its VCC is not.")

    return s


def panel():
    W, H = 900, 480
    s = Svg(W, H, "The button and the status LEDs, pin by pin",
            "Raspberry Pi Pico W pins 27, 26 and 24 to two LEDs through series resistors and to a "
            "momentary button, all returning to pin 38, GND.")

    Y_L1, Y_L2, Y_BTN, Y_GND = 120, 190, 275, 355
    PX, PY, PW, PH = 40, 70, 200, 330
    PR = PX + PW
    RAIL = 700

    s.box(PX, PY, PW, PH, "Raspberry Pi Pico W")
    for y, label in ((Y_L1, "27 GP21"), (Y_L2, "26 GP20"), (Y_BTN, "24 GP18"), (Y_GND, "38 GND")):
        s.pin(PR, y, label, "r")

    for y, which in ((Y_L1, "LED 1, fan 1"), (Y_L2, "LED 2, fan 2")):
        s.wire([(PR, y), (RAIL, y)])
        s.res_inline(400, y, "330 Ω")
        s.led(540, y, which)

    # The button: a gap in the wire, which is all a momentary switch is
    s.wire([(PR, Y_BTN), (455, Y_BTN)])
    s.wire([(495, Y_BTN), (RAIL, Y_BTN)])
    s.add(f'<path d="M455 {Y_BTN} L497 {Y_BTN - 22}" stroke="#3a3a3a" stroke-width="1.6"/>')
    s.add(f'<circle cx="455" cy="{Y_BTN}" r="3.4" fill="#ffffff" stroke="#3a3a3a" stroke-width="1.5"/>')
    s.add(f'<circle cx="495" cy="{Y_BTN}" r="3.4" fill="#ffffff" stroke="#3a3a3a" stroke-width="1.5"/>')
    s.note(475, Y_BTN + 26, "button, momentary", anchor="middle")

    s.wire([(PR, Y_GND), (RAIL, Y_GND)])
    s.wire([(RAIL, Y_L1), (RAIL, Y_GND)])
    for y in (Y_L1, Y_L2, Y_BTN):
        s.dot(RAIL, y)

    s.note(RAIL + 14, Y_GND - 40, "one ground net, back to pin 38")
    s.note(PX, H - 40, "GP18 has the internal pull-up on and the firmware acts on the falling edge, so "
                       "the switch needs no external resistor.")
    s.note(PX, H - 20, "Both LED outputs are active high: the pin drives the anode, the cathode goes "
                       "to ground.")

    return s


def main():
    for name, build in (("wiring-overview", overview), ("wiring-fans", fans),
                        ("wiring-relay", relay), ("wiring-button-leds", panel)):
        with open(f"{name}.svg", "w") as f:
            f.write(build().render())
        print(f"wrote {name}.svg")


if __name__ == "__main__":
    main()
