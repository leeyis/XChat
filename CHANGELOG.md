# Changelog

## 0.1.7 - 2026-10-06

### Fixes

- Embed the Windows frontend from a relative directory so installed clients do not navigate to a temporary build-machine path and show `ERR_FILE_NOT_FOUND`.
- Add a packaging regression check for the embedded entry point and its referenced frontend assets.

### Performance

- Create large-file messages promptly and upload chunks while calculating the checksum when both peers support streaming uploads.
