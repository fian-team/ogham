# Ogham — Text-Input Ergonomics

> **Status: implementation tracker.** Captures the design contract, the staged
> implementation, and the host-seam audit for the text-input / focus ergonomics
> work. Stage status is tracked inline (§4) and updated as steps land.
>
> Decision legend:
> **[DECIDED]** — locked, build toward it.
> **[DEFERRED]** — explicitly out of scope for this effort.
>
> First drafted: 2026-06-08. Defaults locked this session (E1=context-measure,
> E2=char-boundaries, Stage-4=pointer-capture, Stage-5=clipboard-intent).

---

## 1. The reckoning

Almost every text-input weakness traces to one missing capability: **the widget
never measures real glyphs.** The Skia backend already builds a laid-out
`Paragraph` ([`src/skia.rs`](../../src/skia.rs) `build_laid_out_paragraph`) that
wraps, reports height, and answers per-glyph position queries — but that lives
privately inside `draw_text` and is exposed nowhere. So `TextInputWidget`
guesses:

- width: `value.len() * 8.0 + 20.0` ([`text_input_widget.rs`](../../src/widget/text_input_widget.rs) `get_dimensions`)
- height: hard-coded `30.0` (never grows with wrapped lines)
- caret x: `cursor_position * (font_size * 0.55)` (`render`)

And the cursor model is a single byte index used as if it were a char count,
with an `is_ascii_graphic` insert filter — so non-ASCII can't be typed and an
arrow key onto a multibyte boundary panics the next edit.

`LayoutContext` ([`mod.rs`](../../src/widget/mod.rs)) **already carries
`font_collection` + `default_font`** "so that text widgets can measure text" —
the seam is half-built and unused. This work finishes it.

## 2. Locked decisions

- **[DECIDED] E1 — measurement seam.** A backend-agnostic
  `src/widget/text_layout.rs` builds the laid-out paragraph in **logical
  pixels** from `(&FontCollection, default_font, &TextStyle, text, max_width)`.
  Layout reads it via `LayoutContext`; the widget caches the (ref-counted)
  `FontCollection` handle at `layout()` time so `handle_event` (click-to-caret)
  and `render` (caret + scroll) can measure too. `skia.rs` shares the
  ogham→skia `TextStyle` geometry mapping so paint and measurement can't drift.
- **[DECIDED] E2 — selection model.** `cursor_position: usize` →
  `Selection { anchor, caret }` (byte offsets, kept on **char boundaries** —
  std-only, kills the panic and unblocks non-ASCII typing). Collapsed
  (`anchor == caret`) is today's plain caret.
- **[DECIDED] Stage 4 dispatch — pointer capture.** While an input is focused
  and the button is held, route `mouse_move` to it for drag-select. This is
  also the right primitive for future sliders.
- **[DECIDED] Stage 5 — clipboard intent.** Copy/cut/paste route as an intent
  through `EventContext` (mirroring `request_focus`) for the host to fulfil
  against the window owner; `arboard` is the host-side impl. Keeps Ogham
  embeddable under the editable seam (lorekeeper owns the real window).
- **[DECIDED] tab order — tree order.** No new DSL surface; traversal walks the
  live tree in render order. An explicit `tab_index:` prop is a later option.
- **[DEFERRED]** grapheme-cluster movement (emoji/combining), full IME /
  composition, newline entry / multi-line *editing* (display-wrapping of a
  single logical value is in scope; entering `\n` is not),
  undo/redo, password masking, max-length, read-only/disabled, caret blink,
  word-wise nav (Ctrl+←/→), accessibility. (Placeholder text and the
  `text_align`-aware caret left this list on 2026-09-07 — Stage 6.)

## 3. The measurement seam (E1) in detail

`text_layout.rs` exposes, all in logical px:

- `measure(fc, font, style, text, max_width) -> TextMetrics { width, height, line_count }`
- `glyph_index_at(fc, font, style, text, max_width, point) -> usize` (byte index; for click + drag-select)
- `caret_geometry(fc, font, style, text, max_width, byte_index) -> CaretRect { x, top, height }`

Coordinate space: the layout tree and `render`/`draw_line`/`push_clip_rect` all
work in **logical** px and the Skia backend scales internally (`scale_coord` /
`scale_dim`). Measurement therefore builds the paragraph at the **unscaled**
font size + width and reports logical metrics, so caret geometry lines up with
what `draw_text` paints (modulo sub-pixel wrap rounding — acceptable).

---

## 4. Staged implementation

| Stage | Scope | Depends | Status |
|-------|-------|---------|--------|
| **E1** | `text_layout.rs`; skia style-mapping shared; FontCollection cached on widget | — | ✅ done |
| **E2** | `Selection { anchor, caret }`, char-boundary-safe ops, replace-selection | — | ✅ done |
| **1** | UTF-8 movement/insert/delete; drop ascii filter; caret from `caret_geometry`; click-to-caret | E1, E2 | ✅ done |
| **2** | wrap + auto-height (`Shrink` height = measured); clip to box; horizontal scroll-into-view | E1 | ✅ done |
| **3** | on_change fires only on real edits; Tab/Shift-Tab traversal; `on_submit` on Enter; Escape-to-blur | — | ✅ done |
| **4** | shift+arrows extend ✅; Ctrl+A ✅; mouse drag-select via pointer capture ✅ (2026-09-07); double-click-word ☐ | E2 | ◐ (double-click pending on the host pump) |
| **6** | `on_blur`, `align` honoured, `placeholder` | E1 | ✅ done (2026-09-07) |
| **5** | Ctrl+C/X/V via clipboard intent; `arboard` at host | E2 | ☐ |

**As-built notes (E1/E2/1/2):**
- Measurement lives in `src/widget/text_layout.rs`, all logical-px, taking
  `Option<&FontCollection>` with a thread-local fallback. `skia.rs` shares the
  geometry mapping via `configure_geometry`. `TextWidget` (label) keeps its own
  thread-local path untouched — a future consolidation, not done here.
- Skia paragraph `TextIndex` confirmed **UTF-8 byte offsets** (UTF-16 calls are
  suffixed), so `Selection` offsets pass through with no conversion.
- Multi-line contract: `height: Shrink` ⇒ wrap + grow; any other height ⇒
  single line + horizontal scroll (`scroll_x`, kept caret-visible in
  `update_scroll`). `FontCollection` cached on the widget at `layout()`.
- 10 unit tests cover boundary-safe edit/movement incl. `é`/`🦀`; full suite
  (211+) green.

**As-built notes (Stage 3):**
- `is_focusable()` added to the `Widget` trait (TextInput → true).
  `UI::focus_next(reverse)` walks `collect_focusables` (pre-order DFS) over the
  base tree, or the top `focus_stack` portal's subtree when a trap is active,
  and wraps. `call_event` intercepts `keydown` Tab/Shift-Tab → `focus_next`, and
  Escape → blur — **only while an input is focused**, so both pass through to the
  host otherwise (matches the host-seam gate). `on_submit` fires on Enter,
  registered in the builder alongside `on_change`. 5 traversal tests cover
  order / wrap / reverse / nesting / trap confinement.

**As-built notes (Stage 4 mouse + Stage 6, 2026-09-07):**
- **Pointer capture** is `UI`'s (`EVENTS.md` → *Pointer capture*). A widget
  that consumes `mouse_down` calls `ctx.request_capture(self_ref, point)` with
  the point *as it received it*; the UI derives `viewport − local` from that
  and routes every `mouse_down` / `mouse_move` / `mouse_up` to the widget,
  shifted by the offset, until the release. No ancestor walk: the offset is
  taken at the press, which is exact for the length of a drag. `TextInput`
  takes capture on its press, extends the selection on captured moves and
  keeps it on the release — drag-select fell out of capture as predicted.
  Double-click-word still waits on the host pump emitting a distinct event.
- **`on_blur`** fires with the value whichever way focus left: a press
  elsewhere (after that press's own handlers), Tab, Escape, or a focus
  trap restoring what it saved. Every focus move funnels through
  `UI::blur_if_moved`, so the widget never learns *why*. It does **not**
  fire when the widget is reconciled out of the tree — a widget that no
  longer exists has nobody to report to — and a press on the same field
  is not a blur.
- **`align`** was parsed onto `text_style` and read by nothing. A
  single-line field now measures and paints its run at a left origin and
  seats it by `run_offset` — `(content_w − run_w)` for `right`, half that
  for `center` — because the run is laid out at infinite width (it has to
  scroll) and Skia's own alignment at infinite width places a right-aligned
  run at +∞. The offset applies only while the run fits; an overflowing run
  starts at the left and scrolls, whatever the alignment. Click-to-caret,
  the caret, the selection and the scroll all read the same offset. A
  wrapping field keeps `align` on the paragraph, and `text_layout::build_with`
  now lays out at a finite width whenever it is given one (mirroring
  `SkiaEnv::build_laid_out_paragraph`) so the aligned caret measures where
  it paints. `TextInputWidget::caret_rect` exposes the result.
- **`placeholder`** draws in the text style at half its alpha while the
  value is empty and the field is unfocused, seated by the same alignment
  rule. No `placeholder_style`: one vocabulary to keep in step.

**Remaining:**
- **Double-click-word** — the host pump (`input.rs` already detects a
  double click) needs to emit a distinct event.
- **Stage 5 clipboard** — `EventContext` clipboard intent + `UI::call_event`
  drain + host `arboard` impl, and the lorekeeper seam (§5.3). New dependency +
  cross-repo, so genuinely separate.

---

## 5. Host-seam audit (lorekeeper)

The runtime boundary already gates character keys via `claims_character_keys` /
`UI::consumes_character_key` so a focused input swallows typing instead of
letting it reach game hotkeys. Three additions ride the same precedent:

1. **Tab / Escape consumption.** When an input is focused, Tab (traversal) and
   Escape (blur) must be consumed at the UI level *before* the host treats them
   as hotkeys — extend the focused-widget gate analogously.
2. **`on_submit`** (and, since 2026-09-07, **`on_blur`**). New widget
   listeners; surface through the builder like `on_change`. Host handlers
   register the same way.
3. **Clipboard intents.** `EventContext` grows a clipboard request (copy/cut
   text out, paste text in); `UI::call_event` drains it and the host services
   it against the window via `arboard`. The widget never touches a clipboard
   crate directly — Wayland ties the clipboard to the window owner, which is the
   host, not Ogham.

Confirm these three against the lorekeeper input pump during Stage 3/5.
