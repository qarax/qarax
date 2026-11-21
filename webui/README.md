# qarax web UI

A single-page app (SvelteKit, static) for the qarax control plane, built on
PatternFly 6's framework-free CSS (`@patternfly/patternfly`, imported in
`src/lib/patternfly.ts`; markup follows the package's `docs/` examples). qarax-server
serves its build at `/ui` from `ui_dir` (`UI_DIR`, default
`/usr/share/qarax/ui`); the control-plane image ships it there.

The API types in `src/lib/api/schema.d.ts` are generated from
`../openapi.yaml` by `pnpm gen` (`gen-api.js`), which `dev`, `build` and `check` run
first, so a change to the API that the UI doesn't follow fails `pnpm check`.

```sh
pnpm install
pnpm dev     # http://localhost:5173/ui, API from QARAX_URL (default http://localhost:8000)
pnpm check   # types (svelte-check)
pnpm lint    # formatting (prettier); pnpm format fixes it
pnpm build   # the static build, in build/
```

`make run-local` starts a control plane with a node to develop against; sign
in with its token, `e2e-test-token`.
