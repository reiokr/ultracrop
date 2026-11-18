# UltraCrop mechanics — what the original actually does

The rebuild must match the original, so every value below was read out of the
original program, not invented. This sheet is the source of truth for
`app/src/main.rs`.

## Where these numbers come from

- **Original:** `/home/rk/projektid/pythoni_projektid/ultracrop.py`
- **Kind:** Python 3 source, 1775 lines (`file` says “Python script, Unicode
  text, UTF-8 executable”)
- **SHA-256:** `00963123578f627c47a16b083329353ee88b76c3a357191255b98c052026c10e`
- **Size:** 71 724 bytes

### 1. The native decompiler refuses it — re-checked 2026-11-03

UltraCrop is an interpreted Python program. Craft Anything's native engine
reads ELF/PE/Mach-O/DOS machine code and refuses this target, so there is no
pseudo-C and no native function entry address. The exact results, re-run
today, are:

| Probe | Result |
|---|---|
| `core_target_open` | kind `unknown`, description “A file we don't recognise yet”, `raw: null` |
| `native_functions_list` | `not_supported`: “Couldn't read this as a program: the file is malformed (Unknown file magic)” |
| `native_strings_find` (`Kärbitud`) | `not_supported`: same “Unknown file magic” refusal |
| `native_function_pseudocode` (`flip`) | `not_supported`: same “Unknown file magic” refusal |

This is a property of the target, not a skipped step: a Python file has no
machine code to lift.

### 2. So it was decompiled where it really executes — CPython bytecode

The program the user actually runs is the bytecode CPython compiles this file
to, so that is what was decompiled. The original was compiled with the same
interpreter it runs on (**CPython 3.14.9**, cache tag `cpython-314`), every one
of its **180 code objects** was disassembled, and every instruction was recorded
with its offset.

Addresses in this sheet are therefore written:

```
<function>+0x<offset>          e.g.  UltraCrop.flip+0x00CA
```

`<offset>` is the byte offset of that instruction inside the function's own
`co_code` — the exact analogue of a native `flip+0xCA`, and the address of the
instruction that loads the constant, compares the value or emits the string.
Where a rule lives in a module-level constant the address is `<module>+0x…`.

The complete decompiled listing — 180 code objects, 10 826 lines, every
instruction with its address — is committed at:

```
reference/ultracrop.bytecode.txt
```

It is reproducible from the original at any time:

```python
import dis
src = open("ultracrop.py", "rb").read()
mod = compile(src, "ultracrop.py", "exec")
def walk(co, qual):
    print(f"== {qual}  {len(co.co_code)} bytes")
    for ins in dis.get_instructions(co):
        print(f"  {qual}+0x{ins.offset:04X} {ins.opname} {ins.argrepr}")
    for c in co.co_consts:
        if hasattr(c, "co_code"):
            walk(c, c.co_qualname)
walk(mod, "<module>")
```

### 3. How the values below were traced

Every string and number in this sheet was found in the decompiled listing,
followed to the code object that uses it, and that object was read in full
(section 16 lists value → address). The Python source line spans in sections
1–15 are kept as a human-readable cross-check; they are **not** the primary
citation any more.

### 4. What the decompilation corrected in the build (2026-11-03)

| Rule | Decompiled evidence | What the build did | Now |
|---|---|---|---|
| Zoom percentage label | `UltraCrop._update_zoom_label+0x002C` calls `int()`; `+0x0060` multiplies by 100 | rounded (`1.126` → `113%`) | **truncates** (`112%`), `zoom_label_percent()` |
| Crop edges → pixels | `CropCanvas.get_crop_box+0x0096` `int()`, `+0x0232` rejects sides `< 1` | same rule inline, untested | extracted as `crop_box_pixels()` and covered by tests |
| Quick-save numbering | `UltraCrop.quick_save+0x0106` `"_karp"`, `+0x0126` counter starts at 2 | same rule inline, untested | extracted as `quick_save_path()` and covered by tests |
| File-size text | `UltraCrop._fmt_bytes+0x0004` 1 MiB, `+0x002C` 1 KiB | already correct | now pinned by a test |

`cargo test` covers these four rules (8 tests). Everything else in sections
1–12 was already aligned; section 13 lists the differences that remain on
purpose.


---

## 1. Fixed constants

| Rule | Real value | Original location |
|---|---:|---|
| Undo history | **10 steps** | `UNDO_LIMIT`, L73 |
| Minimum crop side | **12 px** | `MIN_BOX`, L72 |
| Handle radius | **7 px** | `HANDLE_HALF`, L67 |
| Handle hit tolerance | **7 + 2 = 9 px**, square test | `CropCanvas._handle_at`, L268–272 |
| Recent files | **8** | `MAX_RECENT`, L66 |
| Zoom minimum (absolute) | **0.05×** | `CropCanvas.__init__`, L126 |
| Zoom maximum (absolute) | **8.0×** | `CropCanvas.__init__`, L127 |
| Wheel step | **×1.12** per notch (÷1.12 down) | `_on_wheel`, L210–218 |
| Zoom button step | **×1.25 / ×0.8** | `_build_toolbar` L923–928, `_bind_keys` L1175–1176 |
| Side panel | **290 px** inner, **320 px** with padding | `PANEL_W`, `SIDE_W`, L68–69 |
| Window chrome allowance | **40 px** wide, **175 px** high | `CHROME_W`, `CHROME_H`, L70–71 |
| Window minimum | **820 × 540** | `root.minsize(820, 540)`, L718 |
| Fit-window height floor | **560 px** | `_compute_fit_size`, L765 |
| Preview box | **234 × 180 px** maximum, shrink only | `_update_preview`, L1409–1419 (`PANEL_W - 56`) |
| Preview filter | **LANCZOS** thumbnail | `_update_preview`, L1417 |
| Default starting crop | **none** — a new image has no box | `CropCanvas.set_image(keep_box=False)`, L150–155 |
| Rule of thirds | **off** by default | `_thirds = False`, L138 |
| Grid | **6 × 6**, off by default | `_grid = False`, L137; `_draw_grid`, L520 |
| Image extensions | png, jpg, jpeg, bmp, gif, tiff, tif, webp, **ico** | `IMAGE_EXTS`, L74 |
| Opened colour mode | **RGB, or RGBA if the source has alpha** | `_open_file`, L1457–1460 |
| Screenshot warm-up | *(not an original feature: 120 frames is the Craft rebuild harness)* | `app/src/main.rs` |

Palette (`ultracrop.py` L47–64), now used verbatim in Rust:

```
BG #0f0f17   SURFACE #1a1a2e   SURFACE2 #16213e   CARD #0f3460
BORDER #1f4068   TEXT #e2e8f0   TEXT_DIM #94a3b8
ACCENT #6366f1   ACCENT_HOVER #4f46e5
GREEN #10b981   AMBER #f59e0b   RED #ef4444   PINK #ec4899   CYAN #06b6d4
```

Canvas background in the original is `SURFACE` (`CropCanvas.__init__`, L121),
not a separate near-black colour.

---

## 2. Crop box geometry

### 2.1 Fit and zoom — `CropCanvas._fit` L157–165, `zoom_by` L186–203

```
fit_scale = min(canvas_width / image_width,
                canvas_height / image_height,
                1.0)                       # never upscales to fit
offset    = (canvas - image * fit_scale) / 2
```

`zoom_by(factor, cx, cy)`:

```
new_zoom = clamp(old_zoom * factor, 0.05, 8.0)
if new_zoom == old_zoom: stop
offset_x = cx - (cx - offset_x) * (new_zoom / old_zoom)
offset_y = cy - (cy - offset_y) * (new_zoom / old_zoom)
```

The Rust `zoom` value is a multiplier on `fit_scale`; `clamp_zoom()` converts
it so the **absolute** scale stays inside 0.05…8.0, as in the original.
`Ctrl+0` is fit (`zoom = 1.0`, `pan = 0`).

### 2.2 Drawing with a ratio — `CropCanvas._on_drag` L329–341

```
dx = ix - x1 ; dy = iy - y1
if abs(dx) > abs(dy) * r:
    dy = abs(dx) / r * sign(dy)
else:
    dx = abs(dy) * r * sign(dx)
box = (x1, y1, x1 + dx, y1 + dy)
```

`r` is the effective ratio (`_eff_ratio`, L229–236):
`Shift` → **1.0**, else the selected aspect, else the free lock ratio.
`drag_rect()` in Rust uses exactly this dominant-axis rule.

### 2.3 Resizing with a ratio — `CropCanvas._do_resize` L357–406

Normalise to `L < R`, `T < B`; move only the dragged edges. With ratio `r`:

- corner handles: anchor = **opposite corner**; the dominant axis wins:
  `dw = |ix - ax|`, `dh = |iy - ay|`;
  `w = dh * r` if `dh * r > dw`, else `w = dw`; `h = w / r`
- `tm` / `bm`: height drives, `h = |B - T|`, `w = h * r`, x stays centred
- `ml` / `mr`: width drives, `w = |R - L|`, `h = w / r`, y stays centred

Rust `resize_rect()` follows these three cases.

### 2.4 Release and reading the box — L408–423, L531–543

- On release the box is normalised.
- If **either side < 12 px** (`MIN_BOX`) the box is discarded (`None`).
- `get_crop_box()` clamps to the image and **truncates with `int()`** to whole
  pixels; it returns `None` if either side is < 1 px.
- The Rust `crop_pixels()` now uses `as u32` (truncation) for min and max,
  matching Python `int()`, and normalises first.

### 2.5 Nudge — `CropCanvas.nudge` L546–552

Arrow = **1 px**, Shift+Arrow = **10 px**, pure translation; no clamping at
nudge time (the box is clamped when read). Rust now translates only.

---

## 3. Overlays

- Dim mask: black `gray25` stipple over the **whole canvas**, four rectangles
  (L451–464). Rust approximates 25 % coverage with `alpha 64`.
- Border: accent `#6366f1`, 2 px, dash **(8, 4)**, plus white 1 px dash
  **(4, 8)** with a 6 px phase offset (L473–477). Rust draws the same two
  dashed rectangles.
- Handles: radius **7**, fill accent, white outline **1.5 px** (L479–483).
- Size label: `int(abs(x2-x1)) × int(abs(y2-y1)) px`, white bold 9, shadowed,
  placed above the top-left corner (or below if it would leave the canvas),
  L485–496. Rust keeps the label format and truncation.
- Thirds: 2 lines each way, white, 1 px, dash **(4, 4)**, 50 % stipple
  (L504–512). Rust uses dashed white alpha 128.
- Grid: 6 columns × 6 rows, 1 px, colour **`#334155`** (L520–528). Rust now
  uses exactly that colour and 6 divisions.

---

## 4. Aspect ratio and lock

- Presets, in order: **Vaba, 1:1, 16:9, 9:16, 4:3, 3:4, 3:2, 2:3, 5:4**
  (L1043–1053).
- Choosing a preset only sets the constraint (`_select_aspect`, L1298–1305);
  it does **not** reshape an existing box. Rust no longer reshapes.
- Lock (`_toggle_lock`, L1307–1324): take the current box ratio if the box is
  at least 1 × 1 px, otherwise the image ratio; with no image, unlock and say
  “Pole pilti — pole midagi lukustada”. Status shows the ratio to 3 decimals.
- Shared status strings:
  - `Kuvasuhe lukustatud ({r:.3})`
  - `Kuvasuhe vabastatud`

---

## 5. Resize image — `_on_resize_w_changed` L1339–1353, `_on_resize_h_changed` L1355–1369, `_apply_resize` L1371–1392

With “link ratio” on:

```
h = max(1, round(w * image_h / image_w))
w = max(1, round(h * image_w / image_h))
```

Apply:

```
nw = max(1, int(text_w))
nh = max(1, int(text_h))
if (nw, nh) == current size: “Suurus on juba sama”
push undo
image = image.resize((nw, nh), LANCZOS)
box  = None
```

Rust uses `resize_exact(..., Lanczos3)` so the colour mode is preserved like
PIL, and clears the box.

---

## 6. Turns and mirrors

- `rotate(90)` = PIL `rotate(-90, expand=True)` = **clockwise 90°**
  (L1635–1644). `rotate(-90)` = counter-clockwise.
- `flip("h")` = `FLIP_LEFT_RIGHT`, `flip("v")` = `FLIP_TOP_BOTTOM`
  (L1646–1657).
- Every one of these calls `set_image(...)` with the default
  `keep_box=False`, so the crop box is cleared afterwards. Rust now clears it
  for flip too.

---

## 7. Undo / redo

- `_push_undo` (L1577–1583): only when an image exists; append a copy; if the
  stack is longer than **10**, drop the oldest; clear redo.
- `undo` / `redo` (L1585–1607): move one image between stacks, show it with
  `keep_box=False`, refresh the size fields.
- Opening a file pushes the old image and then clears both stacks, so a fresh
  image starts with empty history (L1466–1471).

Rust now uses `UNDO_LIMIT = 10` and clears the box on undo/redo.

---

## 8. Saving and exporting

### 8.1 Save dialog — `save_crop` L1667–1686

- Image written: **crop if a box exists, else the full image**
  (`_get_save_image`, L1660–1665).
- Suggested name: **`karpitud` + source extension** — e.g. `karpitud.png`.
- Default extension: the source file’s extension (`or ".png"`).
- Filters, in order: **PNG, JPEG, BMP, TIFF, WebP, all files**.

### 8.2 Quick save — `quick_save` L1688–1703

```
base, ext = splitext(current_file)
if not ext: ext = ".png"
path = base + "_karp" + ext
n = 2
while path exists:
    path = base + "_karp" + n + ext
    n += 1
```

So the second save is `photo_karp2.png`, the third `photo_karp3.png`, and so
on. Rust matches this loop. With no current file the original only reports
“Pilt pole avatud.” — it does **not** fall back to the save dialog; Rust now
does the same.

### 8.3 Encoding — `_write_image` L1705–1713

- JPEG/JPEG paths are converted to **RGB** first (no alpha).
- Everything else is saved in its own format.
- Success status: `Salvestatud: <basename>`.

### 8.4 Clipboard — `copy_to_clipboard` L1716–1746

The original always encodes the crop (or full image) as **PNG** and sends the
bytes to `xclip` (`-selection clipboard -t image/png`), then `xsel` as a
fallback, and finally writes a temporary `.png` and reports its path. The Rust
port uses `arboard::set_image`, which delivers the same PNG image to the
desktop clipboard without shelling out; the fallback-to-temp-file path is not
needed there.

---

## 9. Window and file behaviour

### 9.1 Window fit — `_compute_fit_size` L758–766

```
w = min(max(image_w + 320 + 40, 820), screen_w * 0.88)
h = min(max(image_h + 175,     560), screen_h * 0.86)
```

Size only, never position. The original keeps re-applying this for **4.0 s**
with a **3 px** tolerance, late checks at 250/500/900/1500/2500 ms
(`_fit_window_to_image` L1495–1525, `_schedule_enforce` L1540–1574). The Rust
port reproduces the same bounded enforcement: `enforce_window_fit()` re-sends
`ViewportCommand::InnerSize` (or the fullscreen request) every 150 ms until
the size is within 3 px, the 4.0 s deadline expires, or the user starts
dragging. It also leaves fullscreen and unmaximizes first, because a
maximized window ignores `InnerSize`.

KWin's third-party **“Remember Window Positions”** script can re-apply an old
geometry *after* the app's first resize, including shrinking a fullscreen
window to the previous windowed size. UltraCrop is therefore listed in that
script's blacklist (`~/.config/kwinrc`, group
`Script-rememberwindowpositions`, key `blacklist`). With the blacklist in
place a 1800×1200 image opens at 2160×1375 and a 4000×3000 image covers the
full 3840×2160 screen; without it KWin restores the old 2165×1408 window.

**Owner request (2026-11-03) changes the Rust sizing policy.** The formula
above is still what the original does, but the rebuild now sizes on open like
this (`app/src/main.rs`, `fit_window_to_image`):

| Opened image | Window |
|---|---|
| Needs less than the app's normal **1180 × 780** | normal default size, unchanged |
| Fits on the screen | exactly the image plus its chrome (320 + 40 wide, 175 high) |
| Bigger than the screen (image + chrome exceeds the monitor) | **fullscreen**; the canvas `min(cw/iw, ch/ih, 1.0)` fit shows the whole image inside the app |

`F11` toggles fullscreen by hand, so an automatically-fullscreen image can be
brought back to a normal 1180 × 780 window. The 820 × 540 window minimum is
still enforced by the window manager.


### 9.2 Recent files — L784–808

- Stored in a newline-separated file, most recent first.
- **8** entries (`MAX_RECENT`), duplicates moved to the front, missing files
  dropped on load.
- Entries are absolute paths: `_add_recent` runs `os.path.abspath(path)`
  (L801–802), so a relative argument becomes absolute. The Rust port now does
  the same with `std::path::absolute` in `open_path`.
- Original path: `.ultracrop_recent` next to the script. The Rust port keeps
  the same list/limit but stores it at `~/.config/ultracrop/recent.txt`
  (a per-user location that works for an installed binary).

### 9.3 Open — `_open_file` L1455–1493

- Normalise the mode to **RGB or RGBA** only (`"A" in mode` → RGBA).
- Clear undo/redo, clear the crop box, show size/mode/file-size.
- Title becomes `UltraCrop – <filename>` (en dash).
- File size formatting (`_fmt_bytes`, L1426–1432):
  - `>= 1 MiB`: 2 decimals, `MB`
  - `>= 1 KiB`: 1 decimal, `kB`
  - else bytes, `B`

### 9.4 Single instance — `_send_to_instance` L693–710, `_start_ipc_server` L1213–1276

The original listens on `/tmp/ultracrop-<uid>.sock`; a second launch sends the
file path, receives `OK`, and exits. This is **not ported** in the current
Rust build (not a value or formula; noted as a known gap).

---

## 10. Keyboard map — `_bind_keys` L1159–1190

| Keys | Action |
|---|---|
| Ctrl+O | open |
| Ctrl+S / Ctrl+Shift+S | save / quick save |
| Ctrl+Z / Ctrl+Y | undo / redo |
| Ctrl+X | apply crop |
| Ctrl+C | copy to clipboard |
| Ctrl++ / Ctrl+- | zoom ×1.25 / ×0.8 |
| Ctrl+0 | fit |
| Esc | clear box |
| R / Shift+R | rotate 90° CW / CCW |
| H / V | flip horizontal / vertical |
| Arrows / Shift+Arrows | nudge 1 px / 10 px |

`Shift` while drawing forces ratio 1.0.

---

## 11. Status / UI strings that are part of the behaviour

```
Valmis — ava pilt alustamiseks
Avatud: {name}
Kõigepealt joonista kärpimisraam
Kärbitud → {w} × {h} px
Raam tühjendatud
Pööratud {degrees}°
Peegeldata horisontaalselt
Peegeldata vertikaalselt
Tagasi võetud
Uuesti tehtud
Tagasivõtmise ajalugu on tühi
Uuestitegemise ajalugu on tühi
Kõigepealt ava pilt!
Pilt pole avatud.
Sisesta korrektsed pikslimõõdud
Suurus on juba sama
Suurust muudetud → {nw} × {nh} px
Salvestatud: {basename}
Kuvasuhe lukustatud ({r:.3})
Kuvasuhe vabastatud
Pole pilti — pole midagi lukustada
Kopeeritud lõikelauale (xclip)
Kopeeritud lõikelauale (xsel)
Ajutine fail: {path} (xclip/xsel puudub)
Pildi avamine ebaõnnestus: {error}
Kopeerimine ebaõnnestus: {error}
```

Note the two mirror strings are **“Peegeldata …”**, not “Peegeldatud …” — the
original's wording (L1652, L1655). The rebuild previously had the wrong word;
it now matches. `Kõigepealt ava pilt!` also carries its exclamation mark
(L1669).

---

## 12. Changes applied to the Rust build for this sheet

| Area | Before | Now (original value) |
|---|---|---|
| Palette | custom near-black/indigo | exact original hex |
| Undo limit | 16 | **10** |
| Minimum crop | 10 px | **12 px** |
| Zoom clamp | 0.05…40 | **0.05…8.0 absolute** |
| Wheel | exponential `exp(scroll*0.0016)` | **×1.12 per notch** |
| Handle hit | circle, 11 px | **square, 9 px (7 + 2)** |
| Handle draw | white fill, accent ring, 6.5 | **accent fill, white 1.5 px rim, 7** |
| New image | 8 % inset box | **no box** |
| Aspect preset | reshaped existing box | **only sets constraint** |
| Crop pixels | floor/ceil | **truncate (`int()`)** |
| Flip | kept box | **clears box** |
| Preview | full image when no box, 150 px | **“—” when no box, 234 × 180, shrink only** |
| Save name | `{base}_karp.{ext}` | **`karpitud{ext}`** |
| Quick save, no file | opened save dialog | **status “Pilt pole avatud.”** |
| Grid | white alpha 55 | **#334155, 6×6** |
| Thirds | solid | **4/4 dashed, 50 %** |
| Border | solid + 7/5 dash | **accent 8/4 + white 4/8, phase 6** |
| Dim mask | image only, alpha 150 | **whole canvas, gray25 ≈ alpha 64** |
| Info order | size, mode, file size | **size, file size, mode, crop coords** |
| Recent list | 5 shown | **8** |
| Recent paths | stored as given | **absolute (`os.path.abspath`)** |
| Window fit | fixed 1180 × 780 min 860 × 560 | **image+360 × image+175, caps 88 %/86 %, min 820 × 540** |
| Resize image | forced RGBA | **preserves colour mode** |
| Status default | “ava pilt või lohista…” | **“Valmis — ava pilt alustamiseks”** |
| Title | `UltraCrop · name` | **`UltraCrop – name`** |
| Open colour mode | kept the decoder's mode | **RGB/RGBA normalise on open** |
| ICO files | listed but not decodable | **decoded (`ico` feature enabled)** |
| Zoom − / + buttons | ×1.25 / ×0.8 clamped 0.05…40 | **absolute clamp 0.05…8.0** |
| Empty canvas | “Ava pilt või lohista see siia” + hint | **“Lohista fail siia  ·  Ctrl+O”** |
| Mirror status | “Peegeldatud …” | **“Peegeldata …”** |
| Save with no image | “Kõigepealt ava pilt” | **“Kõigepealt ava pilt!”** |
| Save dialog filters | no all-files entry | **Kõik (\*)** |
| Preview size | literal 234 × 180 | **`PANEL_W − 56` × 180** |
| Side panel | literal 334 | **`PANEL_W + 44`** total, so the card content is the original **290 px** (`PANEL_W`) |
| Open dialog title | “Ava pilt” | **“Vali pilt”** |

---

## 13. Deliberate, disclosed differences

1. **No native machine-code addresses** — the original is interpreted Python,
   so the native engine refuses it. Its real addresses are CPython bytecode
   offsets (`<function>+0x…`), listed in section 16 and in
   `reference/ultracrop.bytecode.txt`.
2. **Single-instance Unix socket** is not implemented in Rust.
3. **Window-size enforcement** keeps the original's 4 s / 3 px window, but
   checks every 150 ms instead of at five fixed instants and stops as soon as
   the user drags. By owner request the rebuild also opens images larger than
   the screen fullscreen (F11 returns to a normal window), while tiny images
   get the normal 1180 × 780 window. KWin's “Remember Window Positions”
   script must have `ultracrop` in its blacklist; otherwise it restores an old
   geometry after the fit (this is a user-config change, not app code).
4. **Recent-file location** is `~/.config/ultracrop/recent.txt` instead of a
   dot-file beside the source.
5. **Clipboard** uses `arboard` instead of `xclip`/`xsel` subprocesses.
6. The Rust port keeps the crop box inside the image while dragging; the
   original lets it overhang and clamps only when reading. Both produce the
   same saved pixels because `crop_pixels()` clamps.
7. The original has no “select all” button; the Rust build keeps it as an
   extra convenience. It selects the full image exactly.
8. **“Ava PhotoCraftis”** is a new feature. It is enabled only while a crop
   box exists, writes that crop to a fresh PNG under
   `$TMPDIR/ultracrop-photocraft/`, and runs `photocraft <file>`; PhotoCraft
   opens the file as a new tab in the running app. Temporary files are left
   for PhotoCraft to read (the OS clears them).
9. **Invalid size text is unreachable.** The original reads the W/H fields as
   text and reports “Sisesta korrektsed pikslimõõdud” when they do not parse
   (`_apply_resize` L1379). The rebuild uses numeric drag fields that cannot
   hold invalid text, so that message never appears; values are clamped to
   1…40000 instead.

---

## 14. Original function index — file addresses

These are **byte offsets of each `def` line inside `ultracrop.py`**, so every
rule above can be located exactly in the original file. They are file
addresses, not runtime or machine-code addresses (the program has none; see
the top of this sheet).

| Function / class | Line | File offset |
|---|---:|---:|
| `CropCanvas` | 117 | `0x00E82` |
| `CropCanvas.__init__` | 120 | `0x00ED0` |
| `CropCanvas.set_image` | 150 | `0x0147B` |
| `CropCanvas._fit` | 157 | `0x0152F` |
| `CropCanvas._on_configure` | 167 | `0x01679` |
| `CropCanvas._i2c` | 173 | `0x0176D` |
| `CropCanvas._c2i` | 176 | `0x017DA` |
| `CropCanvas._clamp_img` | 179 | `0x0184B` |
| `CropCanvas.zoom_by` | 186 | `0x019C1` |
| `CropCanvas.zoom_fit` | 205 | `0x01C44` |
| `CropCanvas._on_wheel` | 210 | `0x01CAD` |
| `CropCanvas.set_aspect` | 221 | `0x01E70` |
| `CropCanvas.set_lock` | 225 | `0x01EE2` |
| `CropCanvas._eff_ratio` | 229 | `0x01F64` |
| `CropCanvas._handle_positions` | 256 | `0x0237D` |
| `CropCanvas._handle_at` | 268 | `0x02529` |
| `CropCanvas._on_press` | 282 | `0x0278E` |
| `CropCanvas._on_drag` | 322 | `0x02CFE` |
| `CropCanvas._do_resize` | 357 | `0x0313D` |
| `CropCanvas._on_release` | 408 | `0x037CB` |
| `CropCanvas._draw_overlay` | 451 | `0x03E01` |
| `CropCanvas._draw_box` | 466 | `0x040A7` |
| `CropCanvas._draw_thirds` | 498 | `0x04553` |
| `CropCanvas._draw_grid` | 514 | `0x047B1` |
| `CropCanvas.get_crop_box` | 531 | `0x04A47` |
| `CropCanvas.nudge` | 546 | `0x04CD4` |
| `_send_to_instance` | 693 | `0x06338` |
| `UltraCrop` | 714 | `0x06676` |
| `UltraCrop.__init__` | 715 | `0x06687` |
| `UltraCrop._compute_fit_size` | 758 | `0x06DD4` |
| `UltraCrop._apply_premap_geometry` | 768 | `0x06F9D` |
| `UltraCrop._recent_path` | 784 | `0x0725F` |
| `UltraCrop._load_recent` | 787 | `0x072D9` |
| `UltraCrop._save_recent` | 794 | `0x073C8` |
| `UltraCrop._add_recent` | 801 | `0x0749D` |
| `UltraCrop._build_body` | 934 | `0x08EB7` |
| `UltraCrop._bind_keys` | 1159 | `0x0B711` |
| `UltraCrop._start_ipc_server` | 1213 | `0x0C188` |
| `UltraCrop._select_aspect` | 1298 | `0x0CC23` |
| `UltraCrop._toggle_lock` | 1307 | `0x0CD66` |
| `UltraCrop._sync_resize_fields` | 1327 | `0x0D0F4` |
| `UltraCrop._on_resize_w_changed` | 1339 | `0x0D269` |
| `UltraCrop._on_resize_h_changed` | 1355 | `0x0D4E7` |
| `UltraCrop._apply_resize` | 1371 | `0x0D765` |
| `UltraCrop._update_crop_info` | 1400 | `0x0DBCC` |
| `UltraCrop._update_preview` | 1409 | `0x0DD0C` |
| `UltraCrop._fmt_bytes` | 1427 | `0x0E056` |
| `UltraCrop._open_file` | 1455 | `0x0E45E` |
| `UltraCrop._fit_window_to_image` | 1495 | `0x0EA55` |
| `UltraCrop._schedule_enforce` | 1540 | `0x0F18A` |
| `UltraCrop._push_undo` | 1577 | `0x0F7B7` |
| `UltraCrop.undo` | 1585 | `0x0F8D2` |
| `UltraCrop.redo` | 1597 | `0x0FADC` |
| `UltraCrop.apply_crop` | 1610 | `0x0FD8C` |
| `UltraCrop.clear_box` | 1627 | `0x1002C` |
| `UltraCrop.rotate` | 1635 | `0x101AC` |
| `UltraCrop.flip` | 1646 | `0x1035B` |
| `UltraCrop._get_save_image` | 1660 | `0x105FE` |
| `UltraCrop.save_crop` | 1667 | `0x10704` |
| `UltraCrop.quick_save` | 1688 | `0x10A37` |
| `UltraCrop._write_image` | 1705 | `0x10C81` |
| `UltraCrop.copy_to_clipboard` | 1716 | `0x10E8D` |

---

## 15. Rule → function ledger (source addresses)

Each row is one mechanic, the Python function that implements it, the exact
line span that was read, and the byte offset of that function's `def` line in
`ultracrop.py` (computed from the file, not guessed). These are the addresses
to cite for this original; there are no machine-code addresses because the
program is interpreted (section at the top).

| Mechanic | Function | Lines | Offset |
|---|---|---|---|
| Palette + numeric constants | module header | 47–74 | `0x005AA` |
| Canvas defaults (bg, zoom 0.05/8.0, grid/thirds off) | `CropCanvas.__init__` | 120–148 | `0x00ED0` |
| New image gets no box | `CropCanvas.set_image` | 150–155 | `0x0147B` |
| Fit scale `min(cw/iw, ch/ih, 1.0)` + centring | `CropCanvas._fit` | 157–165 | `0x0152F` |
| Zoom × factor, clamp 0.05…8.0, cursor anchor | `CropCanvas.zoom_by` | 186–203 | `0x019C1` |
| Wheel notch ×1.12 / ÷1.12 | `CropCanvas._on_wheel` | 210–218 | `0x01CAD` |
| Shift forces ratio 1.0, preset or lock otherwise | `CropCanvas._eff_ratio` | 229–236 | `0x01F64` |
| Handle hit test = 7 + 2 px square | `CropCanvas._handle_at` | 268–272 | `0x02529` |
| Draw-with-ratio dominant-axis formula | `CropCanvas._on_drag` | 322–341 | `0x02CFE` |
| Resize-with-ratio (anchor, edges, corners) | `CropCanvas._do_resize` | 357–406 | `0x0313D` |
| Box discarded below 12 px on release | `CropCanvas._on_release` | 408–423 | `0x037CB` |
| Whole-canvas `gray25` dim mask | `CropCanvas._draw_overlay` | 451–464 | `0x03E01` |
| Border dashes, handles, size label | `CropCanvas._draw_box` | 466–496 | `0x040A7` |
| Thirds: 2 × 2, white 4/4, `gray50` | `CropCanvas._draw_thirds` | 498–512 | `0x04553` |
| Grid 6 × 6, `#334155` | `CropCanvas._draw_grid` | 514–528 | `0x047B1` |
| Crop read: `int()`, clamp, < 1 px → None | `CropCanvas.get_crop_box` | 531–543 | `0x04A47` |
| Nudge 1 px / 10 px, no clamp | `CropCanvas.nudge` | 546–552 | `0x04CD4` |
| Window fit formula (820×540 floor, 88 %/86 % caps) | `UltraCrop._compute_fit_size` | 758–766 | `0x06DD4` |
| Recent files: 8, newest first, missing dropped | `_recent_path`…`_add_recent` | 784–808 | `0x0725F` |
| Aspect preset order and values | `UltraCrop._build_body` | 1043–1053 | inside `0x08EB7` |
| Keyboard map (Ctrl/R/H/V/arrows) | `UltraCrop._bind_keys` | 1159–1190 | `0x0B711` |
| Preset selection only sets the constraint | `UltraCrop._select_aspect` | 1298–1305 | `0x0CC23` |
| Lock ratio from box or image, 3 decimals | `UltraCrop._toggle_lock` | 1307–1324 | `0x0CD66` |
| Resize link `round(w·ih/iw)` / `round(h·iw/ih)` | `_on_resize_w_changed`, `_on_resize_h_changed` | 1339–1369 | `0x0D269` |
| Apply resize: `int()`, min 1, LANCZOS, clear box | `UltraCrop._apply_resize` | 1371–1392 | `0x0D765` |
| Crop info text (clamped ints) | `UltraCrop._update_crop_info` | 1400–1407 | `0x0DBCC` |
| Preview: crop, `thumbnail((PANEL_W-56, 180))`, “—” | `UltraCrop._update_preview` | 1409–1419 | `0x0DD0C` |
| File size: MB 2 dp / kB 1 dp / B | `UltraCrop._fmt_bytes` | 1427–1432 | `0x0E056` |
| Open: RGB/RGBA normalise, clear history, title | `UltraCrop._open_file` | 1455–1493 | `0x0E45E` |
| 4 s window-fit enforcement, 3 px tolerance | `_fit_window_to_image`…`_schedule_enforce` | 1495–1574 | `0x0EA55` |
| Undo 10 deep, clear redo, clear box | `_push_undo`…`redo` | 1577–1607 | `0x0F7B7` |
| Crop / clear / rotate CW-CCW / mirror | `apply_crop`…`flip` | 1610–1657 | `0x0FD8C` |
| Save target, `karpitud{ext}`, `_karp[n]`, JPEG→RGB | `_get_save_image`…`_write_image` | 1660–1713 | `0x105FE` |
| Clipboard: crop PNG → xclip/xsel/temp file | `UltraCrop.copy_to_clipboard` | 1716–1746 | `0x10E8D` |

Status strings are emitted by the functions that perform the action, so their
“address” is the function above: mirror text in `flip` (`0x1035B`; the two
lines are `0x10433` and `0x104CB`), save-empty text in `save_crop`
(`0x10704`, text at `0x1073D`), lock text in `_toggle_lock` (`0x0CD66`),
placeholder text in `CropCanvas.redraw` (`0x03A83`).

---

## 16. Decompiled address ledger — value → bytecode instruction

These are **real addresses in the program that runs**: the byte offset of the
instruction inside each code object of `ultracrop.py` compiled by CPython
3.14.9. Full listing: `reference/ultracrop.bytecode.txt`. Read a row as
“this value is loaded/compared/emitted at this instruction”.

**Re-verified 2026-11-03.** The original was compiled again with the same
interpreter and every code object disassembled: **180 code objects, 9 919
instructions**, and the resulting listing is instruction-for-instruction the
same as the committed one (offsets and opcodes identical). Every address in
this section was then checked against that live disassembly — the instruction
at the cited offset really does load, compare or emit the value in the row.
One citation was corrected by that pass (the `_karp` constant is at
`quick_save+0x0108`, not `+0x0106`), and the `PANEL_W + 44` figure is now
labelled as rebuild layout maths rather than an original constant (the
original parameter panel is `PANEL_W + 16` = 306 px plus 3 + 6 px of outer
padding; `SIDE_W` = 320 is only used for window sizing).

The native engine was re-probed on the same day and still refuses the file
(`not_supported`: “Unknown file magic”), which is expected: a `.py` file has no
machine code. The bytecode listing above is the decompiled form of the program
that actually runs. An independent cross-check of the file offsets in §14–15:
a byte search for `Kuvasuhe lukustatud (` lands at file offset `0x0CF3F` and
`Peegeldata horisontaalselt` at `0x10451` — both inside the `def` ranges cited
for `_toggle_lock` and `flip`.

The numeric constants in 16.1–16.3 are also pinned by a unit test,
`constants_match_the_decompiled_ledger` in `app/src/main.rs`, so a later edit
cannot quietly drift away from the values in this sheet.

### 16.1 Module constants — loaded once at import

| Value | Address | Instruction |
|---|---|---|
| `BG` … `CYAN` palette (24 colours) | `<module>+0x00F0` … `+0x0136` | `LOAD_CONST '#0f0f17'` … `'#06b6d4'` |
| `MAX_RECENT = 8` | `<module>+0x0138` | `LOAD_SMALL_INT 8` |
| `HANDLE_HALF = 7` | `<module>+0x013C` | `LOAD_SMALL_INT 7` |
| `PANEL_W = 290` | `<module>+0x0140` | `LOAD_CONST 290` |
| `SIDE_W = 320` | `<module>+0x0144` | `LOAD_CONST 320` |
| `CHROME_H = 175` | `<module>+0x0148` | `LOAD_SMALL_INT 175` |
| `CHROME_W = 40` | `<module>+0x014C` | `LOAD_SMALL_INT 40` |
| `MIN_BOX = 12` | `<module>+0x0150` | `LOAD_SMALL_INT 12` |
| `UNDO_LIMIT = 10` | `<module>+0x0154` | `LOAD_SMALL_INT 10` |
| `IMAGE_EXTS` (9 extensions) | `<module>+0x0158`…`+0x015E` | `BUILD_SET`, `SET_UPDATE` |
| `SOCK_PATH` template | `<module>+0x005A`, `+0x007E` | `'/tmp/ultracrop-'`, `'.sock'` |

### 16.2 Canvas, zoom and crop box

| Rule / value | Address | Instruction |
|---|---|---|
| start zoom `1.0` | `CropCanvas.__init__+0x005E` | `LOAD_CONST 1.0` |
| `_zoom_min = 0.05` | `CropCanvas.__init__+0x006C` | `LOAD_CONST 0.05` |
| `_zoom_max = 8.0` | `CropCanvas.__init__+0x007A` | `LOAD_CONST 8.0` |
| grid off / thirds off | `…__init__+0x010A`, `+0x0118` | `STORE_ATTR _grid/_thirds` |
| fit fallback canvas `800`, `600` | `CropCanvas._fit+0x005A`, `+0x008E` | `LOAD_CONST 800/600` |
| fit never upscales (`1.0`) | `CropCanvas._fit+0x00E8` | `LOAD_CONST 1.0` |
| clamp low = `_zoom_min` | `CropCanvas.zoom_by+0x00CA` | `LOAD_ATTR _zoom_min` |
| clamp high = `_zoom_max` | `CropCanvas.zoom_by+0x00EA` | `LOAD_ATTR _zoom_max` |
| wheel notch `×1.12` | `CropCanvas._on_wheel+0x0094` | `LOAD_CONST 1.12` |
| wheel notch `÷1.12` | `CropCanvas._on_wheel+0x0098` | `LOAD_CONST 0.8928571428571428` (folded `1/1.12`) |
| Shift forces ratio `1.0` | `CropCanvas._eff_ratio+0x0026` | `LOAD_CONST 1.0` |
| handle hit = `HANDLE_HALF` | `CropCanvas._handle_at+0x006E` | `LOAD_GLOBAL HANDLE_HALF` |
| handle hit `+ 2` px slop | `CropCanvas._handle_at+0x0078` | `LOAD_SMALL_INT 2` |
| draw sign for `dy` | `CropCanvas._on_drag+0x0212` | `LOAD_CONST -1` |
| resize midpoint `÷2` | `CropCanvas._do_resize+0x01BC` | `LOAD_SMALL_INT 2` |
| release rejects `< MIN_BOX` | `CropCanvas._on_release+0x0072`, `+0x007C` | `LOAD_GLOBAL MIN_BOX`, `COMPARE_OP '<'` |
| border width `2` px | `CropCanvas._draw_box+0x00B0` | `LOAD_SMALL_INT 2` |
| border dash phase `6` | `CropCanvas._draw_box+0x00E0` | `LOAD_SMALL_INT 6` |
| handle rim `1.5` px | `CropCanvas._draw_box+0x01B6` | `LOAD_CONST 1.5` |
| label truncates with `int()` | `CropCanvas._draw_box+0x01CC` | `LOAD_GLOBAL int` |
| label `max(cx1, 4)` / `cy1-16` | `…_draw_box+0x0252`, `+0x0260` | `LOAD_SMALL_INT 4`, `16` |
| thirds divisor `3` | `CropCanvas._draw_thirds+0x009A` | `LOAD_SMALL_INT 3` |
| thirds stipple `gray50` | `CropCanvas._draw_thirds+0x0110` | `LOAD_CONST 'gray50'` |
| grid `6 × 6` | `CropCanvas._draw_grid+0x008C` | `LOAD_SMALL_INT 6` |
| grid colour `#334155` | `CropCanvas._draw_grid+0x0126` | `LOAD_CONST '#334155'` |
| dim mask `gray25` | `CropCanvas._draw_overlay+0x00CC` | `LOAD_CONST 'gray25'` |
| clamp floor `0` | `CropCanvas.get_crop_box+0x0096` | `LOAD_SMALL_INT 0` |
| truncate with `int()` | `CropCanvas.get_crop_box+0x00A2` | `LOAD_GLOBAL int` |
| reject side `< 1` px | `CropCanvas.get_crop_box+0x0232` | `LOAD_SMALL_INT 1` |
| placeholder text | `CropCanvas.redraw+0x00E6` | `LOAD_CONST 'Lohista fail siia  ·  Ctrl+O'` |

### 16.3 Window, preview, history

| Rule / value | Address | Instruction |
|---|---|---|
| window `minsize(820, 540)` | `UltraCrop.__init__+0x009A`, `+0x009C` | `LOAD_CONST 820`, `540` |
| zoom label seed `"100%"` | `UltraCrop.__init__+0x01E2` | `LOAD_CONST '100%'` |
| fit floor width `820` | `_compute_fit_size+0x00AC` | `LOAD_CONST 820` |
| fit cap `0.88` of screen width | `_compute_fit_size+0x00C2` | `LOAD_CONST 0.88` |
| fit floor height `560` | `_compute_fit_size+0x010E` | `LOAD_CONST 560` |
| fit cap `0.86` of screen height | `_compute_fit_size+0x0124` | `LOAD_CONST 0.86` |
| fit enforcement `4.0` s | `_fit_window_to_image+0x0162` | `LOAD_CONST 4.0` |
| late checks `(250,500,900,1500,2500)` ms | `_fit_window_to_image+0x01AA` | `LOAD_CONST (250, 500, 900, 1500, 2500)` |
| drift tolerance `3` px | `_schedule_enforce+0x016C` | `LOAD_SMALL_INT 3` |
| preview width `PANEL_W - 56` | `_update_preview+0x00F6` | `LOAD_SMALL_INT 56` |
| preview height `180` | `_update_preview+0x0104` | `LOAD_SMALL_INT 180` |
| label `int(zoom × 100)` | `_update_zoom_label+0x002C`, `+0x0060` | `LOAD_GLOBAL int`, `LOAD_SMALL_INT 100` |
| undo trims at `UNDO_LIMIT` | `_push_undo+0x00B4` | `LOAD_GLOBAL UNDO_LIMIT` |
| recent list trims at `MAX_RECENT` | `_add_recent+0x00E6` | `LOAD_GLOBAL MAX_RECENT` |
| recents stored as absolute paths | `_add_recent+0x0020` | `LOAD_ATTR abspath` |

### 16.4 Editing, saving, clipboard

| Rule / value | Address | Instruction |
|---|---|---|
| resize with no image text | `_apply_resize+0x003C` | `LOAD_CONST 'Pilt pole avatud'` |
| resize unchanged text | `_apply_resize+0x0174` | `LOAD_CONST 'Suurus on juba sama'` |
| resize result text | `_apply_resize+0x0370` | `LOAD_CONST 'Suurust muudetud → '` |
| quick save no image text | `quick_save+0x006C` | `LOAD_CONST 'Pilt pole avatud.'` |
| quick save suffix `_karp` | `quick_save+0x0108` | `LOAD_CONST '_karp'` |
| quick save counter starts `2` | `quick_save+0x0126` | `LOAD_SMALL_INT 2` |
| save with no image text | `save_crop+0x0048` | `LOAD_CONST 'Kõigepealt ava pilt!'` |
| suggested save name `karpitud{ext}` | `save_crop+0x01D4` | `LOAD_CONST 'karpitud'` |
| default extension `.png` | `save_crop+0x00FC` | `LOAD_CONST '.png'` |
| JPEG export → `RGB` | `_write_image+0x0068` | `LOAD_CONST 'RGB'` |
| save error text | `_write_image+0x0134` | `LOAD_CONST 'Salvestamine ebaõnnestus:\n'` |
| `1 MiB` threshold / 2 dp | `_fmt_bytes+0x0004`, `+0x0020` | `LOAD_CONST 1048576`, `'.2f'` |
| `1 KiB` threshold / 1 dp | `_fmt_bytes+0x002C`, `+0x0048` | `LOAD_CONST 1024`, `'.1f'` |
| open normalises `"A"` → RGBA | `_open_file+0x0068`, `+0x008A` | `LOAD_CONST 'A'`, `'RGBA'` |
| open normalises otherwise RGB | `_open_file+0x008E` | `LOAD_CONST 'RGB'` |
| window title dash | `_open_file+0x03E8` | `LOAD_CONST 'UltraCrop – '` |
| mirror h / v status | `flip+0x00CA`, `+0x014A` | `'Peegeldata horisontaalselt'` / `'…vertikaalselt'` |
| rotate status | `rotate+0x01A6`, `+0x01AC` | `'Pööratud '`, `'°'` |
| lock status, 3 decimals | `_toggle_lock+0x0248`, `+0x024C` | `'Kuvasuhe lukustatud ('`, `'.3f'` |
| no-image lock status | `_toggle_lock+0x02AE` | `'Pole pilti — pole midagi lukustada'` |
| crop with no box text | `apply_crop+0x0084` | `'Kõigepealt joonista kärpimisraam'` |
| clear status | `clear_box+0x00DE` | `'Raam tühjendatud'` |
| clipboard `(xclip)` / `(xsel)` | `copy_to_clipboard+0x0140`, `+0x01BE` | matching `LOAD_CONST` |
| empty undo / redo text | `undo+0x003C`, `redo+0x003C` | matching `LOAD_CONST` |

### 16.5 Key map and IPC

| Rule / value | Address | Instruction |
|---|---|---|
| `Ctrl+O` | `_bind_keys+0x0032` | `LOAD_CONST '<Control-o>'` |
| `Ctrl+S` / `Ctrl+Shift+S` | `+0x008A`, `+0x00E2` | `'<Control-s>'`, `'<Control-Shift-s>'` |
| `Ctrl+Z` / `Ctrl+Y` | `+0x013A`, `+0x0192` | `'<Control-z>'`, `'<Control-y>'` |
| `Ctrl+X` / `Ctrl+C` | `+0x01EA`, `+0x0242` | `'<Control-x>'`, `'<Control-c>'` |
| `Ctrl++` / `Ctrl+-` / `Ctrl+0` | `+0x029A`, `+0x02C6`, `+0x02F2` | `'<Control-equal>'`, `'<Control-minus>'`, `'<Control-0>'` |
| `r` / `R` / `h` / `v` | `+0x034A`, `+0x0376`, `+0x03A2`, `+0x03CE` | arrow-key and letter binds |
| arrows / Shift+arrows | `+0x03FA` … `+0x052E` | `'<Up>'` … `'<Shift-Right>'` |
| socket connect timeout `1.0` s | `_send_to_instance+0x0080` | `LOAD_CONST 1.0` |
| ack read `16` bytes | `_send_to_instance+0x010C` | `LOAD_SMALL_INT 16` |
| ack is `b"OK"` | `_send_to_instance+0x0138` | `LOAD_CONST b'OK'` |
| listen backlog `8` | `_start_ipc_server+0x00E6` | `LOAD_SMALL_INT 8` |
| poll every `150` ms | `_start_ipc_server+0x0180` | `LOAD_SMALL_INT 150` |

Aspect presets are a single tuple constant: `'Vaba', ('1:1', (1,1)),
('16:9', (16,9)), ('9:16', (9,16)), ('4:3', (4,3)), ('3:4', (3,4)),
('3:2', (3,2)), ('2:3', (2,3)), ('5:4', (5,4))` at
`UltraCrop._build_body+0x065A`; `_select_aspect` only forwards the ratio to
`set_aspect` at `_select_aspect+0x010A`.
