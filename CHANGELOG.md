# Changelog

## 0.1.11 - 2026-10-08

### Features

- Share the capture editor across Web and desktop with full-screen selection, multiline text editing, movable annotations, and wheel-adjustable drawing tools.
- Add independent pinned images with history, grouping, transforms, a hover-only status hint, and a window-shadow toggle.
- Use a light, consistent annotation toolbar and a compact completion icon.

### Fixes

- Keep selection and text positions in source-image coordinates across display scaling, editing, dragging, and export.
- Commit text when clicking outside, insert new lines with Enter, and support deleting the active text annotation with Delete.
- Show the configured capture shortcut in the workspace and preserve native pin dimensions and transparent borders.

### Performance

- Reduce lossless PNG encoding and Windows cursor-compositing work during capture.
- Avoid duplicate source reads, image decoding, and canvas copies; load capture windows independently of the main chat interface.

### Tests

- Stabilize temporary database cleanup on Windows and resolver timeout tests under heavy CPU load.
- Verify Web capture, Windows screen/cursor capture, native global shortcuts, clipboard output, pinned images, and embedded frontend resources.

## 0.1.7 - 2026-10-06

### Fixes

- Embed the Windows frontend from a relative directory so installed clients do not navigate to a temporary build-machine path and show `ERR_FILE_NOT_FOUND`.
- Add a packaging regression check for the embedded entry point and its referenced frontend assets.

### Performance

- Create large-file messages promptly and upload chunks while calculating the checksum when both peers support streaming uploads.
