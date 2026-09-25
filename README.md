# KinNovel

KinNovel is a LightNovelShelf client for jailbroken Kindle e-readers. It follows
the kComics runtime model: Python 3.14 for Kindle HF, Pillow rendering directly
to `/dev/fb0`, EPDC ioctl updates, and evdev touch input.

## Features

- LightNovelShelf login, registration, password reset and token refresh.
- Latest books, category browsing, rankings, paged lists and all server search
  modes (fuzzy, exact, title, author, series and tags).
- Book details, chapter catalogue, reading progress and history.
- Novel reader with server-provided per-chapter fonts, manual pagination,
  footnotes, illustrations, night mode, font size and line spacing settings.
- Remote shelf browsing, adding/removing books, folder creation/deletion and
  basic local/remote synchronization.
- Announcements, comments, notifications, daily sign-in, points overview and
  shop item purchase.
- Chinese, English and numeric on-device input using the adapted pinyin keyboard.

## Deliberate exclusions

Uploading, publishing, editing, downloading/exporting, forum/community,
manga image reading, avatar upload and direct-message UI are not implemented.
They either require heavy editors, large image buffers, or provide low value on
an e-ink device. The API client still contains typed calls for the implemented
read-only account flows.

## Install

Prerequisites match kComics:

1. A jailbroken Kindle on firmware 5.16.3 or newer.
2. Python 3.14 for Kindle HF installed under `/mnt/us/python3`.
3. KUAL or a KPM-compatible launcher.

From a package root:

```sh
./install.sh
```

The installer copies the extension to `/mnt/us/extensions/kinnovel`. In
`config.json`, set `screen_protocol` to `mtk` or `mxcfb` if automatic detection
does not work on a particular device.

## Development

Run unit tests:

```sh
PYTHONPATH=bin/src python -m unittest discover -s tests -v
```

Render desktop previews without touching Kindle devices:

```sh
python tools/render_preview.py
```

Previews are written to `build/previews`.

## License

GPLv3. See `LICENSE` and `THIRD-PARTY-NOTICES.md`.
