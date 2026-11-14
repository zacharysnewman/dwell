# Pending release notes

A PR says what it changed here instead of raising `client/package.json` and editing
`shared/releases/changes.json` (docs/RELEASES.md §3, "Pending release notes"): one file,
`<slug>.json` (the branch name works: lower case letters, digits, `.`, `_`, `-`), with one to six
short lines for players:

```json
{ "changes": ["Lava glows at night", "Behind the scenes: faster water tests"] }
```

After the merge, the Release workflow on `main` gives each note the next patch
(`npm run release:assign`, `client/scripts/releaseAssign.ts`), writes its `changes.json` entry and
`package.json`, deletes the note, commits that to `main` and releases it. Several notes merged
together each get a version; the newest is built, the others are listed under it as "Earlier".

`"breaking": true` is refused (CI fails on it): a new compatibility line is raised by hand
(`package.json` to the next MINOR, `shared/version/line.json`, its `changes.json` entry).
