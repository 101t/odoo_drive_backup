# Contributing

Thanks for helping improve Odoo Backup. Issues and pull requests are welcome.

## Before you start

- Search the existing issues first. For a larger change, open an issue to discuss it before writing code.
- Security problems go through [SECURITY.md](SECURITY.md), not public issues.

## Development setup

You need a recent stable Rust toolchain and Node.js (see `.github/workflows/ci.yml` for the versions CI uses).

```bash
make dev-backend     # API on :8080 (data in ./data)
make dev-frontend    # Vite on :5173 with hot reload, proxies /api to :8080
```

## Checks

A pull request must pass what CI runs:

```bash
make lint            # rustfmt and clippy
make test            # cargo test and the TypeScript type-check
```

To run the database tests against PostgreSQL as well, set `ODB_TEST_POSTGRES_URL` (see the README).

## Pull requests

- Keep each pull request focused on one change, and add or update tests for behaviour changes.
- Use [Conventional Commits](https://www.conventionalcommits.org/) for commit messages (`feat:`, `fix:`, `docs:`…).
- Database changes need a migration in both `backend/migrations/sqlite/` and `backend/migrations/postgres/`.
- Never commit secrets, backups or real Odoo data.

## Translations

The UI catalogs live in `frontend/src/i18n/`. `en.ts` is the source; the other languages are typed against it, so a
missing key fails the type-check.

1. Copy `fr.ts` to `<code>.ts` and translate the values (keep `{placeholders}` unchanged).
2. Register the language in `frontend/src/i18n/index.ts` (the `Locale` type, `catalogs` and `LANGUAGES`).
3. Run `npm run typecheck` in `frontend/`.

Improvements to existing translations are welcome too. Server-generated texts (run logs, API errors) are translated
only for Arabic, in `server.ts`.

## License

By contributing, you agree that your contributions are licensed under the [MIT License](LICENSE).
