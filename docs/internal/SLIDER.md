# Ogham — Slider

> **Status: live contract. Built 2026-09-07.**
>
> The first native control that needs the pointer after the cursor has
> left it, and the first taker of pointer capture (`EVENTS.md` →
> *Pointer capture*). Authority: `src/widget/slider_widget.rs`; builder
> in `src/widget/builder.rs` (`create_slider_widget`); vocabulary in
> `src/widget/vocabulary.rs` (`SLIDER_PROPERTIES`); tests in
> `tests/slider.rs`.

## Why a widget

untold_lore's editing surface composed a "slider" out of a filled bar
and two nudge buttons (`ul-editor/data/ui/widgets.ogh`), with the
comment that the runtime had no drag-thumb primitive and a drag-heavy
slider would be a host Rust widget. It could not be composed:
`mouse_move` was hover-only in `UI::call_event` and never reached a
listener, and a drag has to keep tracking after the cursor leaves the
track. Both are a dispatch question, not a layout one, so the answer
is a primitive in the runtime rather than a host widget.

## Contract

```ogh
Slider {
  value: settings.volume,                     // required, number; controlled
  min: 0, max: 100, step: 5,                  // step optional, positive
  on_change: fn (v: float) { event("set_volume", v); },   // while held
  on_commit: fn (v: float) { event("save_volume", v); },  // on release
  style: { width: "grow", height: 16 },
  track_color: { r: 60, g: 60, b: 66, a: 255 },
  fill_color:  { r: 90, g: 140, b: 210, a: 255 },
  thumb_color: { r: 245, g: 245, b: 245, a: 255 },
}
```

| | |
|---|---|
| Orientation | Horizontal only. |
| `value` | **Controlled.** The document supplies it every render; the widget reports what the pointer asked for and adopts the document's value on every reconcile. A document that does not write the report back sees the thumb snap to what it did supply. Snapped through the widget's own grid and range on the way in. |
| `min` / `max` | Numbers, default `0` / `1`. A reversed pair is honoured. |
| `step` | Optional; snaps reports to `min + n·step`, clamped. `0` or negative is a `BridgeError`. |
| The track | The content box: the style's box less margin and padding. A press at a fraction of its width is that fraction of `min..max`. |
| The thumb | Drawn at the value, held inside the box at the ends (the track maps the whole width; only the drawing is clamped). Diameter is the content height. |
| `on_change` | Once per change while the button is held, and on a key step. Argument is the value, `Value::Float`. |
| `on_commit` | On the release, and after a key step. Argument as above. |
| Keyboard | Left / Right while focused: one `step` (a hundredth of the range without one), `on_change` then `on_commit`. A slider is focusable and takes focus on its press. |
| Sizing | `shrink` is 160 × 16 plus insets; `grow` and fixed sizes as any leaf. |
| Style | The `Flex` style vocabulary. `background_color`, `border` and `corners` paint the box; the rest of the box model lays out. |
| Occlusion | Always consumes a press inside itself (`blocks_point`). |
| Reconcile | Absorbs in place; listeners swap like every other widget's. |

The value rides `Event::payload` to its listeners, the channel the drag
events already use, so no field was added to `Event`.

## Dispatch

A press inside the box takes pointer capture with the point as the
slider received it. Every move until the release is routed to the
slider by `UI` with that translation applied, whether or not the cursor
is anywhere near the track — `ctx.is_captured(self_ref)` is what lets
`handle_event` accept a point outside its rect. The release reports the
final value, commits, and ends the capture.

`tests/slider.rs` drives this end to end: a press at 25 % of the track
reports `min + 0.25·(max − min)`; a move to 75 % while held reports
again; a move to 300 px below the track still reports; the release
commits; a step snaps; and a document that never writes the report back
is shown to snap the thumb on the next frame.

## What the build corrected against the brief

- The brief allowed `on_change` to carry the value however fitted. It
  rides `Event::payload`, because `Event::value` is a `String` and a
  new numeric field on `Event` would have been a third channel.
- Keyboard stepping was a nice-to-have; it is fifteen lines and shipped.
- Colour property names: `track_color`, `fill_color`, `thumb_color`,
  as the brief suggested. They are root properties (not style keys),
  checked as colour maps by the vocabulary scan.
