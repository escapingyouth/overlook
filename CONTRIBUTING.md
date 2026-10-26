# Contributing

## Setup

```sh
npm install        # also points git at .githooks via the prepare script
npm run tauri dev
```

## Commits

Every commit is one line in [Conventional Commits](https://www.conventionalcommits.org) form:

```
<type>(<scope>): <subject>
```

- Imperative, lowercase subject, no trailing period, 72 characters at most.
- One feature or fix per commit.
- Types: `feat`, `fix`, `perf`, `refactor`, `style`, `test`, `docs`, `build`, `ci`, `chore`, `revert`.
- Scopes: `core` (Rust collector and storage), `app` (Tauri shell), `ui` (React interface), `dev` (tooling for development), `brand` (icons and assets), `deps`, `repo`.

## Hooks

| Hook | Runs |
|---|---|
| `commit-msg` | Rejects headers that don't match the convention |
| `pre-commit` | Type-check and UI tests when front-end files are staged; core tests and an app check when Rust files are staged |
| `pre-push` | Type-check, UI tests and the full Rust test suite |
