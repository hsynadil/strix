"""Generate the Strix (owl) app icon at 1024x1024 as a flat, geometric mark."""
from PIL import Image, ImageDraw

S = 1024
img = Image.new("RGBA", (S, S), (0, 0, 0, 0))
d = ImageDraw.Draw(img)

BG = (14, 17, 22, 255)        # --bg
BLUE = (68, 147, 248, 255)    # --accent
BLUE_DK = (46, 104, 184, 255) # darker shade for wing/body accents
AMBER = (217, 164, 65, 255)   # --warm
CREAM = (230, 237, 243, 255)  # --text (eye whites)

# Background: rounded square, matching the app's dark theme.
pad = 24
d.rounded_rectangle([pad, pad, S - pad, S - pad], radius=210, fill=BG)

cx, cy = S / 2, S / 2 + 20

# --- Owl body (wide rounded shape) ---
body_w, body_h = 560, 460
body_top = cy + 40
d.rounded_rectangle(
    [cx - body_w / 2, body_top - body_h / 2, cx + body_w / 2, body_top + body_h / 2],
    radius=200,
    fill=BLUE,
)

# --- Ear tufts (short, rounded triangles) ---
d.polygon(
    [(cx - 190, cy - 230), (cx - 250, cy - 380), (cx - 110, cy - 260)],
    fill=BLUE,
)
d.polygon(
    [(cx + 190, cy - 230), (cx + 250, cy - 380), (cx + 110, cy - 260)],
    fill=BLUE,
)

# --- Head ---
head_r = 300
d.ellipse([cx - head_r, cy - head_r - 60, cx + head_r, cy + head_r - 60], fill=BLUE)

# --- Eyes (large, "monitor" style glowing rings) ---
eye_dx = 165
eye_cy = cy - 90
eye_r_outer = 140
eye_r_iris = 96
eye_r_pupil = 42

for sign in (-1, 1):
    ex = cx + sign * eye_dx
    d.ellipse([ex - eye_r_outer, eye_cy - eye_r_outer, ex + eye_r_outer, eye_cy + eye_r_outer], fill=CREAM)
    d.ellipse([ex - eye_r_iris, eye_cy - eye_r_iris, ex + eye_r_iris, eye_cy + eye_r_iris], fill=BG)
    d.ellipse([ex - eye_r_pupil, eye_cy - eye_r_pupil, ex + eye_r_pupil, eye_cy + eye_r_pupil], fill=AMBER)
    # small glint
    gr = 16
    d.ellipse([ex - eye_r_pupil + 14, eye_cy - eye_r_pupil + 10, ex - eye_r_pupil + 14 + gr, eye_cy - eye_r_pupil + 10 + gr], fill=CREAM)

# --- Beak ---
beak_top = eye_cy + 60
d.polygon(
    [(cx - 44, beak_top), (cx + 44, beak_top), (cx, beak_top + 90)],
    fill=AMBER,
)

# --- Wing accents on the body (subtle darker chevrons, tucked closer in) ---
for sign in (-1, 1):
    wx = cx + sign * 235
    d.polygon(
        [
            (wx, body_top - 10),
            (wx + sign * 55, body_top + 130),
            (wx, body_top + 220),
            (wx - sign * 15, body_top + 130),
        ],
        fill=BLUE_DK,
    )

# --- Feet (two small triangles at bottom) ---
foot_y = body_top + body_h / 2 - 10
for sign in (-1, 1):
    fx = cx + sign * 90
    d.polygon(
        [(fx - 40, foot_y), (fx + 40, foot_y), (fx, foot_y + 60)],
        fill=AMBER,
    )

out_path = "C:/Users/hsyna/AppData/Local/Temp/claude/C--GitHub-perf-diag/23cf2962-0e46-45c8-ac29-2b8c34117350/scratchpad/strix-icon-source.png"
img.save(out_path)
print("saved", out_path, img.size)
