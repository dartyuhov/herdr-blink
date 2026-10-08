# Contributing

Use Conventional Commit titles for your commits and pull requests. The PR title
becomes the squash commit on `main` and determines the next release version.

## Commit and PR titles

Write titles as `type(scope): description`. The scope is optional; add `!`
before the colon when the change breaks compatibility.

| Example title | Version change |
| --- | --- |
| `fix: correct agent ordering` | Patch: `0.1.1` → `0.1.2` |
| `feat(search): add a new filter` | Minor: `0.1.1` → `0.2.0` |
| `feat!: change configuration format` | Major: `0.1.1` → `1.0.0` |
| `docs: explain search syntax` | No release on its own |
| `chore: update tooling` | No release on its own |

Accepted types are `feat`, `fix`, `docs`, `chore`, `refactor`, `perf`, `test`,
`build`, `ci`, and `revert`. Use a lowercase type and a short, specific
description. Mark breaking changes explicitly and explain them in the PR body.

Install the local commit-message check with Python 3 available:

```sh
sh scripts/install-hooks.sh
```

GitHub also checks PR titles. If you use an existing custom hooks directory,
integrate `.githooks/commit-msg` there; the setup script won't replace it.

## Versioning and releases

Release Please manages versions and release notes. You don't need to edit
version files or create tags for an ordinary contribution.

1. Open a PR with a Conventional Commit title and wait for CI to pass.
2. Merge it using a squash merge with the PR title as the commit title.
3. Release Please opens or updates a release PR with version changes and a
   changelog. Several changes can be included in one release.
4. A maintainer merges the release PR when ready. After all four platform
   builds pass, the workflow publishes the tag, GitHub Release page, binaries,
   and checksums.

The highest-impact change determines the version bump: a breaking change takes
priority over a feature, and a feature takes priority over a fix. Until the new
release is published, installation uses the latest complete published release.

See the [README's release guide](README.md#releases) for workflow details and
the [development commands](README.md#development) for local checks.
