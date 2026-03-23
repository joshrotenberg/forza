# Node.js project context

## Language and toolchain

This is a Node.js project. Check `package.json` for the package manager (`npm`, `yarn`, or `pnpm`) and available scripts.

## Common commands

```bash
npm install          # install dependencies
npm run build        # compile / bundle
npm test             # run tests
npm run lint         # lint
npm run format       # format code
npm run typecheck    # TypeScript type check (if applicable)
```

Replace `npm` with `yarn` or `pnpm` as appropriate for the project.

## Conventions

- Check `package.json` `"scripts"` for the exact test and lint commands used in CI.
- If TypeScript is present (`tsconfig.json`), type-check before opening a PR.
- Keep dependencies pinned in `package-lock.json` / `yarn.lock` / `pnpm-lock.yaml`.
- Do not commit `node_modules/`.
- Follow the existing style (tabs vs spaces, semicolons) — defer to the formatter config (`.eslintrc`, `prettier.config.js`, etc.).

## Project layout

Check `package.json` for `"main"`, `"scripts"`, and `"workspaces"` to understand the project structure.
