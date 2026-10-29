# Contributing

Thanks for helping with meme-arr. Bug reports, ideas and pull requests are welcome.

- **Found a bug or want a feature?** Open an issue. Say which version you run (shown in the
  sidebar), whether you use Plex or Jellyfin, and what you expected. The **System -> Logs**
  and **Health** pages usually have the answer; see
  [Troubleshooting](docs/TROUBLESHOOTING.md) first.
- **Security problem?** Please don't open a public issue; see [Security](docs/SECURITY.md).
- **Changing code?** [docs/DEVELOPMENT.md](docs/DEVELOPMENT.md) covers setup, the four checks to
  run (`cargo fmt --all --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test --all`,
  and `npm test` in `web/`), the mock library for trying things without your own media, and the PR
  workflow. One logical change per pull request, with a short description of what you tested.
- **Changing the docs?** Edit `docs/` (the wiki is generated from it), and keep screenshots in
  `docs/images/` generated from the mock library, never from a real one.

By contributing you agree your work is released under the [MIT license](LICENSE).
