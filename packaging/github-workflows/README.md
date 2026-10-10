# GitHub workflows (parked here for now)

`ci.yml` runs clippy and the tests in an Arch container on every push. `release.yml` builds the Arch package
when you push a `v*` tag and attaches `facet-x86_64.pkg.tar.zst` and its checksum to the release, which is
what `install.sh` downloads for the fast, prebuilt install.

They live here, not in `.github/workflows/`, only because GitHub refuses pushes that touch that folder
unless the login has the `workflow` scope. To switch them on, once your token has that scope
(`gh auth refresh -h github.com -s workflow`):

```bash
mkdir -p .github && git mv packaging/github-workflows .github/workflows
git rm .github/workflows/README.md
git commit -m "Enable CI and release workflows" && git push
```

Until a release is published, `install.sh` falls back to building from source.
