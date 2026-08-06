# Icons

[Lucide](https://lucide.dev), ISC licensed — see `LICENSE`.

Vendored rather than loaded from a CDN. The app has to work with no network:
the whole point is a local library you can browse and launch offline, and a
button that renders as a broken image when the internet is down is worse than
a text glyph.

## Using one

The webview loads these as plain files relative to the UI root:

```html
<button class="icon-btn" title="Settings">
  <img src="icons/settings.svg" alt="" width="18" height="18" />
</button>
```

Lucide strokes use `currentColor`, so an icon inherits the surrounding text
colour and needs no per-theme variant. Set `width`/`height` explicitly — the
source files are 24×24 and will render at that size otherwise.

## Adding one

Fetch the file from the Lucide repository rather than hand-editing an existing
icon, so it stays consistent with the set and updates cleanly:

```sh
curl -sSfLO --output-dir ui/icons \
  https://raw.githubusercontent.com/lucide-icons/lucide/main/icons/<name>.svg
```

## What is here

Reserved for the UI work rather than all in use yet:

| icon | intended use |
|---|---|
| `settings` | the gear that opens Settings — currently the `⚙` glyph |
| `x` | close buttons — currently `×` |
| `play`, `download` | game actions |
| `search`, `list`, `grid-2x2` | library controls |
| `arrow-left`, `arrow-right` | navigation |
| `star` | favourites |
| `house`, `folder`, `info` | shell and detail views |
| `refresh-cw` | resync with the server |
| `monitor`, `gamepad-2` | platform and emulator settings |
