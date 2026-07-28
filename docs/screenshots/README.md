# Screenshots

Drop PNGs here with these exact filenames — the root [`README.md`](../../README.md)
already links to them.

| File | View | How to get there |
|---|---|---|
| `01-session-list.png` | Session list | Launch with a few saved sessions and at least one external `caffeinate` running |
| `02-form.png` | New session form | `n`, type a name, tab to `System (-s)` and `Space`, tab to `Timeout (-t)` and `Space`, type `7200` |
| `03-pid-picker.png` | PID picker | In the form: pick `Wait for PID (-w)`, tab to `[ pick from running processes ]`, `Space`, then type to filter |
| `04-details.png` | Session details | Select a session, `Enter` |
| `05-help.png` | Help modal | `?` |

## Capture recipe

Aim for a **110×30** terminal — that is the size every ASCII rendering in the
README was captured at, and it is wide enough that no column truncates.

```bash
# Seed a few sessions so the list isn't empty
mkdir -p ~/.config/protein
cat > ~/.config/protein/sessions.json <<'JSON'
[
  {"id":"00000000-0000-4000-8000-000000000001","pid":null,"name":"Movie Mode",
   "flags":{"display":true,"idle":true,"disk":false,"system":false,"user_active":false},
   "target":{"Timeout":7200},"status":"Stopped","started_at":"2026-01-01T12:00:00+00:00","expires_at":null},
  {"id":"00000000-0000-4000-8000-000000000002","pid":null,"name":"Compile Rust",
   "flags":{"display":false,"idle":true,"disk":true,"system":false,"user_active":false},
   "target":{"Command":"cargo build --release"},"status":"Stopped","started_at":"2026-01-01T12:00:00+00:00","expires_at":null},
  {"id":"00000000-0000-4000-8000-000000000003","pid":null,"name":"Old Backup",
   "flags":{"display":false,"idle":true,"disk":false,"system":false,"user_active":false},
   "target":"Indefinite","status":"Stopped","started_at":"2026-01-01T12:00:00+00:00","expires_at":null}
]
JSON

# Give it an external session to display, and one with a timeout for the progress bar
caffeinate &
caffeinate -i -t 900 &

cargo run --release
```

Then `Cmd+Shift+4` + `Space` to capture the window, or `Cmd+Shift+5` for a region.

Afterwards, clean up the processes you started for the shot:

```bash
kill %1 %2
```

## Notes

- Use a font with box-drawing and block glyphs (Nerd Font, JetBrains Mono, SF Mono
  — all fine). If glyphs render as tofu, capture with `--ascii` instead and name
  the file `01-session-list-ascii.png`.
- The palette is Catppuccin Mocha, so a dark terminal background matches the app's
  own `#1e1e2e` and the screenshot won't have a mismatched frame.
- The `on battery` note and the `no effect on battery` hint next to `System (-s)`
  only appear on battery power — unplug first if you want them in the shot.
