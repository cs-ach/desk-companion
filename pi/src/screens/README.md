# Device shell

The 800×480 shell that owns the screen. Plugins supply data; they do not draw
chrome and they do not own the window.

```
main.slint         Shell — composes the below, exposes the whole API
tokens.slint       colours, type ramp, radii, and every layout number
icon.slint         stroke icons as paths — no asset files
glass.slint        Panel (navigation glass) / Surface (content) / Pill / Dot
dock.slint         the floating dock (shell chrome, never unmounts)
status_bar.slint   the 698x34 row above the slot area
slot.slint         one grid cell, and all five of its states
grid.slint         4x2 widget grid placement
switcher.slint     app launcher — 4 x 2 icon grid, paged, with page dots
settings.slint     dock position and background
fault.slint        shown in the slot area when a plugin screen will not build
```

Driven from Rust by [`pi/src/screen.rs`](../screen.rs), which compiles these
through `slint-interpreter` — the same mechanism plugin components will use, so
there is one rendering path rather than two.

```rust
let screen = Screen::new()?;
screen.set_dock(&items);
screen.set_status("Tuesday, 16 September", false, true, "4 plugins");
screen.set_slots(&slots);
screen.set_apps(&all_apps);
screen.on_slot_activated(|id| println!("slot: {id}"));
screen.run()?;
```

## Geometry

Every number is derived from the insets, not chosen independently. Changing
`edge` or `content_x` changes the cell size, so redo the arithmetic before
touching them — these all divide evenly, and fractional pixels show up as
seams between panels once Slint rounds them.

```
content    698 × 452  @ 88, 14      (content_x is 14 when the dock is right)
status     698 ×  34  @ 88, 14
slot area  698 × 406  @ 88, 60
cell       164 × 196   (4 × 2, gap 14)
dock        64 × 370  @ 12, 55      (right: x 724)
```

The dock sizes itself: `items.length * 56 + 146`, vertically centred. Four
entries above the divider (main plus three plugins) and two below it (app
switcher, settings) gives 370.

A full-page plugin view is the 4×2 span — same rect, same origin. There is one
layout path, not two.

## Running it

```
cargo run -p pi --example shell -- --check      # compile only, print diagnostics
cargo run -p pi --example shell -- --geometry   # derived numbers, both dock sides
cargo run -p pi --example shell                 # dash with demo state
cargo run -p pi --example shell -- --switcher   # the app switcher
cargo run -p pi --example shell -- --settings   # dock position and background
cargo run -p pi --example shell -- --fault      # the fault overlay
cargo run -p pi --example shell -- --opaque     # reduce-transparency fallback
cargo run -p pi --example shell -- --right --wallpaper dusk
```

`--check` is the fast feedback loop: it compiles `shell.slint` and prints Slint
diagnostics without opening a window.

## The glass

An acrylic material: blurred backdrop, saturation boost, semi-transparent tint,
fine grain on top. Slint has no `backdrop-filter` and cannot tile a noise
texture, so the whole material is composited once in Rust when the wallpaper
loads (`pi/src/wallpaper.rs`) and handed to Slint as two finished images. Each
glass element then draws exactly one image — clipped to its own rounded rect,
offset by `-absolute-position` so the grain and the blur stay aligned to the
screen rather than to the element.

| image             | blur | saturate | tint             | used by                            |
|-------------------|------|----------|------------------|------------------------------------|
| `backdrop_menu`   | 22px | 1.40     | `#16161b` at 52% | dock, status chips                 |
| `backdrop_panel`  | 3px  | 1.25     | `#0b0d12` at 62% | widget slots, settings, fault card |

Grain is a hash of the pixel coordinate at ±6/255, applied after the tint so it
sits on top the way Acrylic layers it. Content carries the heavier tint because
it holds small text; the dock is glassier because it holds icons.

The rim is three inset highlights rather than one line — `rim_top` white 50%
along the top edge, `rim_hairline` white 10% around the whole inner edge,
`rim_bottom` white 6% along the bottom — over a 180° tint gradient from white 6%
to grey 6%.

`Tokens.text_2` and `text_3` lift to 72% and 56% when a wallpaper is active.
Secondary text at 50% white disappears against a translucent panel over a bright
photo, and darkening the glass to compensate would defeat the material.

`Screen::set_reduce_transparency(true)` drops the sampling entirely and gives
every surface an opaque card — the accessibility fallback. Try it with
`--opaque`.

Glass still belongs to the navigation layer conceptually — it cannot sample
other glass, so nothing translucent may overlap anything else translucent. The
dock floats over content and never over another glass element.

**Overlays go dark, not light.** Over a photo a `white 5%` surface is
unreadable. The wallpaper carries a top-and-bottom gradient dim and launcher
labels get their own pill. Check any change against `pi/wallpapers/fractal.png`,
which is deliberately the worst case.

**Wallpapers** are any PNG or JPEG in `pi/wallpapers/`; the file stem is the id
the settings picker reports. **Plugin icons** are `plugins/<id>/icon.png` by
convention — no manifest entry — falling back to the built-in glyph and the
entry's fill colour when absent. PNG only for now: Slint's SVG support sits
behind a feature flag this build does not guarantee.

**Manrope is not bundled.** `Tokens.face` names it; if the font is not installed
on the device, Slint silently falls back to the platform sans and the type ramp
drifts. Install it on the Pi image, or embed it and point `Tokens.face` at the
embedded family.

## Slot states

`SlotView` renders all five itself. The shell never substitutes one for another,
because `stale` and `error` are different failures and the difference is the
part worth showing.

| `state`   | meaning                                        |
|-----------|------------------------------------------------|
| `empty`   | mounted, `init()` has not returned              |
| `loading` | first tick in flight, nothing cached            |
| `ready`   | fresh value from the last tick                  |
| `stale`   | tick failed or link down — cached value, dimmed |
| `error`   | plugin panicked, or the widget failed to build  |

## Threading

`Shell` inherits `Window`, so the one Slint event loop belongs to whichever
thread calls `run()` — the main thread. Plugin sandboxes run on their own
threads and must never touch the instance; they send state across a channel and
the main thread applies it.

`main.rs` is **not** wired to this yet — it still blocks on `link::listen`. That
swap is the next step and it is what forces the link onto its own thread.
