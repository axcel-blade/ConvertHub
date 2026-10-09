# Translating ConvertHub

All interface text lives in `src/locales/<code>.json`. The Rust backend never
sends English sentences: it sends a key and variables (for example
`{"key": "errors.outputExists", "vars": {"path": "..."}}`), and the frontend
looks the key up in the active language. Adding a language therefore needs no
code changes.

## Add a language

1. Copy `src/locales/en.json` to `src/locales/<code>.json`, using a BCP-47
   code such as `de`, `pt-BR` or `zh-CN`.
2. Set `_meta.name` to the language's own name (for example "Deutsch"), and
   set `_meta.dir` to `rtl` for right-to-left scripts.
3. Translate the values. Keep the keys and `{placeholders}` unchanged.
4. Run `npm run check:i18n`. It reports keys that English doesn't have.
   Missing keys fall back to English automatically.
5. The language then appears in the language picker. ConvertHub picks the
   operating-system language on first start if a matching file exists.

Format names (MP4, PNG, ...) and tool names (FFmpeg, ...) are not translated.

## Reference

FormatFactory's translation-help page
(<https://www.pcfreetime.com/formatfactory/transhelp.php?language=en>) is linked
from the app ("Help translate") as a reference for how volunteer language
contributions are organized. ConvertHub does not copy its content; contribute
ConvertHub translations as pull requests that add a locale file.
